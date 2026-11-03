// SPDX-License-Identifier: GPL-3.0-or-later
//! Text, PDF, and audio import. Audio is decoded locally and passed to whisper-cli.

use super::router::hide_console;
use crate::{
    config::Settings,
    dialogue::{self, clean_text, reject_secrets},
};
use std::{
    fs::File,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub fn extract_pdf(path: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|_| "não foi possível ler o PDF".to_string())?;
    dialogue::check_file_len(meta.len())?;
    let document =
        lopdf::Document::load(path).map_err(|_| "não foi possível ler o PDF".to_string())?;
    let pages: Vec<u32> = document.get_pages().keys().copied().collect();
    if pages.is_empty() {
        return Err("este PDF não tem texto selecionável".into());
    }
    let text = document
        .extract_text(&pages)
        .map_err(|_| "este PDF não tem texto selecionável".to_string())?;
    if text.trim().is_empty() {
        return Err("este PDF não tem texto selecionável".into());
    }
    if text.chars().count() > dialogue::MAX_TEXT_CHARS {
        return Err("texto grande demais".into());
    }
    reject_secrets(&text)?;
    Ok(clean_text(&text))
}

pub fn transcribe(
    audio: &Path,
    settings: &Settings,
    content_root: &Path,
    data_root: &Path,
) -> Result<String, String> {
    let roots = super::roots(content_root, data_root);
    let cli = find_file(
        &settings.whisper_cli,
        &["whisper-cli", "whisper-cli.exe"],
        &roots,
        None,
    )
    .ok_or_else(|| {
        "não encontrei o whisper-cli. Coloque o executável ao lado do programa".to_string()
    })?;
    let model = find_file(
        &settings.whisper_model,
        &["ggml-small.bin", "ggml-base.bin", "whisper.bin"],
        &roots,
        Some("bin"),
    )
    .ok_or_else(|| "não encontrei o modelo do Whisper (ggml-small.bin)".to_string())?;
    let wav = super::speech::temp_path("wav");
    decode_to_wav(audio, &wav)?;
    let result = run_whisper(&cli, &model, &wav);
    let _ = std::fs::remove_file(&wav);
    let text = result?;
    reject_secrets(&text)?;
    if text.trim().is_empty() {
        Err("não consegui transcrever o áudio".into())
    } else {
        Ok(clean_text(&text))
    }
}

fn run_whisper(cli: &Path, model: &Path, wav: &Path) -> Result<String, String> {
    let mut command = Command::new(cli);
    command
        .arg("-m")
        .arg(model)
        .arg("-f")
        .arg(wav)
        .arg("-l")
        .arg("pt")
        .arg("--no-timestamps")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut command);
    let child = command
        .spawn()
        .map_err(|_| "não foi possível abrir o whisper-cli".to_string())?;
    let finished = child
        .wait_with_output()
        .map_err(|_| "o whisper-cli foi interrompido".to_string())?;
    if !finished.status.success() {
        let detail = String::from_utf8_lossy(&finished.stderr);
        return Err(format!(
            "o whisper-cli falhou. {}",
            dialogue::redact(&detail)
        ));
    }
    let text = String::from_utf8_lossy(&finished.stdout);
    let cleaned = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('['))
        .collect::<Vec<_>>()
        .join(" ");
    Ok(cleaned)
}

pub fn decode_to_wav(source: &Path, destination: &Path) -> Result<(), String> {
    let meta = std::fs::metadata(source).map_err(|_| "não foi possível ler o áudio".to_string())?;
    dialogue::check_file_len(meta.len())?;
    if let Ok(samples) = decode_symphonia(source) {
        return write_f32_wav(destination, &samples);
    }
    if ffmpeg_to_wav(source, destination) {
        return Ok(());
    }
    let extension = source
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if extension == "opus" {
        Err("não consegui ler este Opus. Instale o ffmpeg ou converta para wav, mp3, flac, ogg ou m4a".into())
    } else {
        Err("não consegui ler este áudio. Formatos lidos direto: wav, mp3, flac, ogg e m4a".into())
    }
}

fn decode_symphonia(path: &Path) -> Result<Vec<f32>, String> {
    use symphonia::core::{
        audio::SampleBuffer, codecs::DecoderOptions, errors::Error as AudioError,
        formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
    };
    let file = File::open(path).map_err(|_| "não foi possível ler o áudio".to_string())?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
        hint.with_extension(extension);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|_| "formato de áudio não lido".to_string())?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| "áudio sem trilha".to_string())?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|_| "não foi possível decodificar o áudio".to_string())?;
    let mut mono = Vec::<f32>::new();
    let mut rate = track.codec_params.sample_rate.unwrap_or(16_000);
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(AudioError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(AudioError::DecodeError(_)) => continue,
            Err(_) => break,
        };
        rate = decoded.spec().rate;
        let spec = *decoded.spec();
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buffer.copy_interleaved_ref(decoded);
        let channels = spec.channels.count().max(1);
        for frame in buffer.samples().chunks(channels) {
            let sum: f32 = frame.iter().copied().sum();
            mono.push(sum / channels as f32);
        }
        if mono.len() > rate as usize * 60 * 10 {
            return Err("áudio longo demais".into());
        }
    }
    if mono.is_empty() {
        return Err("áudio vazio".into());
    }
    Ok(resample(&mono, rate, 16_000))
}

fn resample(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == 0 || from_rate == to_rate {
        return input.to_vec();
    }
    let ratio = f64::from(from_rate) / f64::from(to_rate);
    let length = (input.len() as f64 / ratio).floor() as usize;
    let mut out = Vec::with_capacity(length);
    for index in 0..length {
        let position = index as f64 * ratio;
        let left = position.floor() as usize;
        let fraction = (position - left as f64) as f32;
        let a = input.get(left).copied().unwrap_or(0.0);
        let b = input.get(left + 1).copied().unwrap_or(a);
        out.push(a + (b - a) * fraction);
    }
    out
}

fn write_f32_wav(path: &Path, samples: &[f32]) -> Result<(), String> {
    if samples.len() > 16_000 * 60 * 10 {
        return Err("áudio longo demais".into());
    }
    let pcm: Vec<u8> = samples
        .iter()
        .flat_map(|sample| {
            let value = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
            value.to_le_bytes()
        })
        .collect();
    super::speech::write_wav_bytes(path, &pcm, 16_000)
}

fn ffmpeg_to_wav(source: &Path, destination: &Path) -> bool {
    let mut command = Command::new("ffmpeg");
    command
        .arg("-y")
        .arg("-i")
        .arg(source)
        .arg("-ac")
        .arg("1")
        .arg("-ar")
        .arg("16000")
        .arg("-f")
        .arg("wav")
        .arg(destination)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide_console(&mut command);
    command
        .status()
        .map(|status| status.success() && destination.is_file())
        .unwrap_or(false)
}

pub fn find_file(
    configured: &str,
    names: &[&str],
    roots: &[PathBuf],
    extension: Option<&str>,
) -> Option<PathBuf> {
    let configured = configured.trim();
    if !configured.is_empty() {
        let path = PathBuf::from(configured);
        return (path.is_file() && !is_ollama(&path)).then_some(path);
    }
    let mut found = Vec::new();
    for root in roots {
        for dir in [
            root.clone(),
            root.join("models"),
            root.join("tools"),
            root.join("voices"),
        ] {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() || is_ollama(&path) {
                    continue;
                }
                let file_name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default();
                let named = names
                    .iter()
                    .any(|name| file_name == name.to_ascii_lowercase());
                let matches_extension = extension.is_some_and(|ext| {
                    path.extension()
                        .is_some_and(|have| have.eq_ignore_ascii_case(ext))
                        && (names.is_empty() || file_name.contains("qwen") || named)
                });
                if named || matches_extension {
                    found.push(path);
                }
            }
        }
    }
    found.sort();
    found
        .iter()
        .find(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_ascii_lowercase().contains("qwen"))
                .unwrap_or(false)
        })
        .cloned()
        .or_else(|| found.into_iter().next())
        .or_else(|| on_path(names))
}

fn on_path(names: &[&str]) -> Option<PathBuf> {
    let finder = if cfg!(windows) { "where" } else { "which" };
    for name in names {
        let mut command = Command::new(finder);
        command
            .arg(name)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        hide_console(&mut command);
        let Ok(output) = command.output() else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let path = PathBuf::from(line.trim());
            if path.is_file() && !is_ollama(&path) {
                return Some(path);
            }
        }
    }
    None
}

fn is_ollama(path: &Path) -> bool {
    path.to_string_lossy()
        .to_ascii_lowercase()
        .contains("ollama")
}
