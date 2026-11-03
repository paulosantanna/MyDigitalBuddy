// SPDX-License-Identifier: GPL-3.0-or-later
//! Optional language, voice, and file import. The spiking core does not call this.

mod media;
mod router;
mod speech;

use crate::{
    config::Settings,
    dialogue::{PromptMessage, clean_text},
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
};

pub use speech::choose_import;

pub struct TurnRequest {
    pub pet: String,
    pub messages: Vec<PromptMessage>,
    pub settings: Settings,
    pub content_root: PathBuf,
    pub data_root: PathBuf,
}

pub enum Event {
    Reply {
        pet: String,
        text: Result<String, String>,
    },
    Heard {
        pet: String,
        text: Result<String, String>,
    },
    Knowledge {
        pet: String,
        source: String,
        text: Result<String, String>,
    },
}

enum JobKind {
    Reply,
    Heard,
    Knowledge { source: String },
}

struct Job {
    id: u64,
    pet: String,
    kind: JobKind,
    slot: Arc<Mutex<Option<Result<String, String>>>>,
}

pub struct Runtime {
    pub status: String,
    generation: u64,
    cancel: Arc<AtomicBool>,
    job: Option<Job>,
    llama: Arc<Mutex<Option<router::LocalProcess>>>,
    speech_child: Arc<Mutex<Option<std::process::Child>>>,
    speech_generation: Arc<AtomicU64>,
    speech_note: Arc<Mutex<Option<String>>>,
    recorder: Option<speech::Recorder>,
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            status: String::new(),
            generation: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            job: None,
            llama: Arc::new(Mutex::new(None)),
            speech_child: Arc::new(Mutex::new(None)),
            speech_generation: Arc::new(AtomicU64::new(0)),
            speech_note: Arc::new(Mutex::new(None)),
            recorder: None,
        }
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    pub fn recording(&self) -> bool {
        self.recorder.is_some()
    }

    pub fn poll(&mut self) -> Option<Event> {
        if let Some(error) = self
            .speech_note
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            self.status = error;
        }
        let ready = {
            let job = self.job.as_ref()?;
            job.slot
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()?
        };
        let job = self.job.take()?;
        if job.id != self.generation {
            return None;
        }
        if self.status.ends_with('…') {
            self.status.clear();
        }
        Some(match job.kind {
            JobKind::Reply => Event::Reply {
                pet: job.pet,
                text: ready,
            },
            JobKind::Heard => Event::Heard {
                pet: job.pet,
                text: ready,
            },
            JobKind::Knowledge { source } => Event::Knowledge {
                pet: job.pet,
                source,
                text: ready,
            },
        })
    }

    pub fn start_reply(&mut self, request: TurnRequest) {
        let messages = request.messages;
        let settings = request.settings;
        let content_root = request.content_root;
        let data_root = request.data_root;
        self.spawn(
            request.pet,
            JobKind::Reply,
            if settings.cloud_deepseek {
                "Perguntando a vários modelos na NVIDIA. A primeira resposta fica…"
            } else {
                "Pensando com o modelo local…"
            },
            move |cancel, llama| {
                router::complete(
                    &messages,
                    &settings,
                    &content_root,
                    &data_root,
                    llama,
                    cancel,
                )
            },
        );
    }

    pub fn start_knowledge(&mut self, request: TurnRequest, source: String, audio: PathBuf) {
        let settings = request.settings;
        let content_root = request.content_root;
        let data_root = request.data_root;
        self.spawn(
            request.pet,
            JobKind::Knowledge { source },
            "Transcrevendo áudio…",
            move |_cancel, _llama| media::transcribe(&audio, &settings, &content_root, &data_root),
        );
    }

    pub fn start_mic(&mut self) -> Result<(), String> {
        if self.recorder.is_some() {
            return Ok(());
        }
        self.recorder = Some(speech::Recorder::start()?);
        self.status = "Ouvindo…".into();
        Ok(())
    }

    pub fn finish_mic(&mut self, request: TurnRequest) {
        let Some(recorder) = self.recorder.take() else {
            return;
        };
        let settings = request.settings;
        let content_root = request.content_root;
        let data_root = request.data_root;
        match recorder.finish() {
            Ok(pcm) => {
                let path = speech::temp_path("wav");
                if let Err(error) = speech::write_pcm_wav(&path, &pcm) {
                    self.status = error;
                    return;
                }
                self.spawn(
                    request.pet,
                    JobKind::Heard,
                    "Transcrevendo…",
                    move |_cancel, _llama| {
                        let text = media::transcribe(&path, &settings, &content_root, &data_root);
                        let _ = std::fs::remove_file(&path);
                        text
                    },
                );
            }
            Err(error) => self.status = error,
        }
    }

    pub fn speak(&mut self, text: String, pet: String, roots: Vec<PathBuf>) {
        let id = self.speech_generation.fetch_add(1, Ordering::Relaxed) + 1;
        self.kill_speech_child();
        speech::stop_playback();
        let child_slot = Arc::clone(&self.speech_child);
        let generation = Arc::clone(&self.speech_generation);
        let note = Arc::clone(&self.speech_note);
        thread::spawn(move || {
            if generation.load(Ordering::Relaxed) != id {
                return;
            }
            if let Err(error) = speech::speak(&text, &pet, &roots, &child_slot, &generation, id) {
                *note.lock().unwrap_or_else(|err| err.into_inner()) = Some(error);
            }
        });
    }

    pub fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.cancel.store(true, Ordering::Relaxed);
        self.job = None;
        self.stop_speech();
        if let Some(recorder) = self.recorder.take() {
            let _ = recorder.finish();
        }
        self.status.clear();
    }

    pub fn stop_local(&mut self) {
        router::stop_local(&self.llama);
    }

    pub fn halt(&mut self) {
        self.cancel();
        self.stop_local();
    }

    pub fn stop_speech(&mut self) {
        self.speech_generation.fetch_add(1, Ordering::Relaxed);
        self.kill_speech_child();
        speech::stop_playback();
    }

    fn kill_speech_child(&self) {
        if let Some(mut child) = self
            .speech_child
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn spawn(
        &mut self,
        pet: String,
        kind: JobKind,
        status: &str,
        work: impl FnOnce(&AtomicBool, &Mutex<Option<router::LocalProcess>>) -> Result<String, String>
        + Send
        + 'static,
    ) {
        if self.job.is_some() {
            self.status = "ainda estou ocupado".into();
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        self.cancel.store(false, Ordering::Relaxed);
        let id = self.generation;
        let slot = Arc::new(Mutex::new(None));
        let worker_slot = Arc::clone(&slot);
        let cancel = Arc::clone(&self.cancel);
        let llama = Arc::clone(&self.llama);
        self.status = status.into();
        thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if cancel.load(Ordering::Relaxed) {
                    Err("interrompido".into())
                } else {
                    work(&cancel, &llama)
                }
            }));
            let result = match result {
                Ok(value) => value,
                Err(_) => Err("a conversa foi interrompida por um erro interno".into()),
            };
            *worker_slot
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(result);
        });
        self.job = Some(Job {
            id,
            pet,
            kind,
            slot,
        });
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.halt();
    }
}

pub fn import_document(path: &Path) -> Result<(String, String), String> {
    let source = crate::dialogue::source_label(path);
    let text = match crate::dialogue::classify_import(path)? {
        crate::dialogue::ImportKind::Text => crate::dialogue::read_text_document(path)?,
        crate::dialogue::ImportKind::Pdf => media::extract_pdf(path)?,
        crate::dialogue::ImportKind::Audio => {
            return Err("áudio".into());
        }
    };
    let text = clean_text(&text);
    Ok((source, text))
}

pub fn key_status(settings: &Settings) -> Result<(), String> {
    let _keys = router::load_nvapi_keys(settings)?;
    Ok(())
}

pub fn roots(content: &Path, data: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = executable_dir() {
        out.push(dir);
    }
    out.push(content.to_path_buf());
    out.push(data.to_path_buf());
    out
}

pub fn executable_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}
