// SPDX-License-Identifier: GPL-3.0-or-later
//! Local voice. Sophia uses Thalita (`voices/sophia-speak.py` + `sophia.voice.json`).
//! Other profiles use Piper when a model is present, otherwise the Windows voice.

use super::media::find_file;
use super::router::hide_console;
use crate::dialogue::{self, prosody};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub fn speak(
    text: &str,
    pet_id: &str,
    roots: &[PathBuf],
    slot: &Mutex<Option<Child>>,
    generation: &AtomicU64,
    id: u64,
) -> Result<(), String> {
    let text = dialogue::fluid_speech(text);
    let text: String = text.chars().take(2500).collect();
    if text.trim().is_empty() {
        return Err("não há fala para ouvir".into());
    }
    if generation.load(Ordering::Relaxed) != id {
        return Ok(());
    }
    let voice = prosody(pet_id);
    if pet_id == "sophia" {
        if speak_sophia(&text, roots, slot, generation, id).is_ok() {
            return Ok(());
        }
        return speak_system(&text, voice, slot, generation, id);
    }
    if let Some(model) = voice_model(pet_id, roots)
        && let Some(piper) = find_file("", &["piper", "piper.exe"], roots, None)
        && speak_piper(
            &text,
            &piper,
            &model,
            voice.length_scale,
            slot,
            generation,
            id,
        )
        .is_ok()
    {
        return Ok(());
    }
    speak_system(&text, voice, slot, generation, id)
}

fn voice_model(pet_id: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    if !crate::config::valid_pet_id(pet_id) {
        return None;
    }
    let relative = format!("voices/{pet_id}.onnx");
    roots
        .iter()
        .map(|root| root.join(&relative))
        .find(|path| path.is_file())
}

fn speak_sophia(
    text: &str,
    roots: &[PathBuf],
    slot: &Mutex<Option<Child>>,
    generation: &AtomicU64,
    id: u64,
) -> Result<(), String> {
    let script = sophia_speak_script(roots)
        .ok_or_else(|| "não encontrei voices/sophia-speak.py".to_string())?;
    let text_file = temp_path("txt");
    std::fs::write(&text_file, text).map_err(|_| "não foi possível preparar a voz".to_string())?;
    let wav = temp_path("wav");
    let mut command = python_command(&script, &[&text_file, &wav])?;
    hide_console(&mut command);
    let child = command
        .spawn()
        .map_err(|_| "não foi possível abrir a voz da Sophia (python e edge-tts)".to_string())?;
    let synth = park_child(child, slot, generation, id);
    let _ = std::fs::remove_file(&text_file);
    synth?;
    if generation.load(Ordering::Relaxed) != id {
        let _ = std::fs::remove_file(&wav);
        return Ok(());
    }
    if !wav.is_file() {
        let _ = std::fs::remove_file(&wav);
        return Err("a voz da Sophia não gerou áudio".into());
    }
    let played = play_wav(&wav);
    let _ = std::fs::remove_file(&wav);
    played
}

fn sophia_speak_script(roots: &[PathBuf]) -> Option<PathBuf> {
    roots
        .iter()
        .map(|root| root.join("voices/sophia-speak.py"))
        .find(|path| path.is_file())
        .or_else(|| find_file("", &["sophia-speak.py"], roots, None))
}

fn python_command(script: &Path, args: &[&Path]) -> Result<Command, String> {
    fn probe(program: &str, prefix: &[&str]) -> bool {
        let mut cmd = Command::new(program);
        for part in prefix {
            cmd.arg(part);
        }
        cmd.arg("-c")
            .arg("import sys")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
    let work = script
        .parent()
        .ok_or_else(|| "voices/sophia-speak.py inválido".to_string())?;
    let build = |program: &str, prefix: &[&str]| {
        let mut command = Command::new(program);
        for part in prefix {
            command.arg(part);
        }
        command
            .arg(script)
            .args(args)
            .current_dir(work)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    };
    if probe("python", &[]) {
        return Ok(build("python", &[]));
    }
    if probe("py", &["-3"]) {
        return Ok(build("py", &["-3"]));
    }
    Err("instale Python 3 com edge-tts, numpy e ffmpeg no PATH".into())
}

fn speak_piper(
    text: &str,
    piper: &Path,
    model: &Path,
    length_scale: f32,
    slot: &Mutex<Option<Child>>,
    generation: &AtomicU64,
    id: u64,
) -> Result<(), String> {
    let wav = temp_path("wav");
    let mut command = Command::new(piper);
    command
        .arg("--model")
        .arg(model)
        .arg("--output_file")
        .arg(&wav)
        .arg("--length-scale")
        .arg(format!("{length_scale:.2}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide_console(&mut command);
    let mut child = command
        .spawn()
        .map_err(|_| "não foi possível abrir o piper".to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    park_child(child, slot, generation, id)?;
    if generation.load(Ordering::Relaxed) != id {
        let _ = std::fs::remove_file(&wav);
        return Ok(());
    }
    let played = play_wav(&wav);
    let _ = std::fs::remove_file(&wav);
    played
}

fn speak_system(
    text: &str,
    voice: dialogue::Prosody,
    slot: &Mutex<Option<Child>>,
    generation: &AtomicU64,
    id: u64,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        let ssml = temp_path("xml");
        let script = temp_path("ps1");
        std::fs::write(&ssml, utf8_bom(&dialogue::prosody_ssml(text, voice)))
            .map_err(|_| "não foi possível preparar a voz".to_string())?;
        std::fs::write(&script, SYSTEM_VOICE_SCRIPT)
            .map_err(|_| "não foi possível preparar a voz".to_string())?;
        let mut command = Command::new("powershell");
        command
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&script)
            .arg(if voice.feminine { "f" } else { "m" })
            .arg(&ssml)
            .arg(voice.sapi_rate.clamp(-10, 10).to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        hide_console(&mut command);
        let child = command
            .spawn()
            .map_err(|_| "não foi possível abrir a voz do sistema".to_string())?;
        let result = park_child(child, slot, generation, id);
        let _ = std::fs::remove_file(&ssml);
        let _ = std::fs::remove_file(&script);
        result
    }
    #[cfg(not(windows))]
    {
        let _ = (text, voice, slot, generation, id);
        Err("a voz do sistema está disponível no Windows. Coloque voices/<perfil>.onnx e o piper ao lado do programa".into())
    }
}

#[cfg(test)]
pub fn scale_wav_rate(path: &Path, factor: f32) -> Result<(), String> {
    if !factor.is_finite() || !(0.5..=2.0).contains(&factor) || (factor - 1.0).abs() < 0.02 {
        return Ok(());
    }
    let mut data = std::fs::read(path).map_err(|_| "não foi possível ler a voz".to_string())?;
    let pos = data
        .windows(4)
        .position(|mark| mark == b"fmt ")
        .ok_or("áudio inválido")?;
    if data.len() < pos + 20 {
        return Err("áudio inválido".into());
    }
    let rate = u32::from_le_bytes(data[pos + 12..pos + 16].try_into().unwrap_or([0; 4]));
    let byte_rate = u32::from_le_bytes(data[pos + 16..pos + 20].try_into().unwrap_or([0; 4]));
    if rate == 0 || byte_rate == 0 {
        return Err("áudio inválido".into());
    }
    let scaled = |value: u32| ((value as f32) * factor).round() as u32;
    data[pos + 12..pos + 16].copy_from_slice(&scaled(rate).to_le_bytes());
    data[pos + 16..pos + 20].copy_from_slice(&scaled(byte_rate).to_le_bytes());
    std::fs::write(path, data).map_err(|_| "não foi possível ajustar a voz".to_string())
}

fn park_child(
    child: Child,
    slot: &Mutex<Option<Child>>,
    generation: &AtomicU64,
    id: u64,
) -> Result<(), String> {
    *slot.lock().unwrap_or_else(|error| error.into_inner()) = Some(child);
    loop {
        if generation.load(Ordering::Relaxed) != id {
            if let Some(mut child) = slot
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
            {
                let _ = child.kill();
                let _ = child.wait();
            }
            return Ok(());
        }
        let finished = slot
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_mut()
            .and_then(|child| child.try_wait().ok())
            .flatten()
            .is_some();
        if finished {
            let _ = slot
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take();
            return Ok(());
        }
        thread::sleep(Duration::from_millis(40));
    }
}

const SYSTEM_VOICE_SCRIPT: &str = r#"Add-Type -AssemblyName System.Speech
$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
$voices = @($synth.GetInstalledVoices() | Where-Object { $_.Enabled } | ForEach-Object { $_.VoiceInfo })
$culture = @($voices | Where-Object { $_.Culture.Name -like 'pt*' })
$gender = if ($args[0] -eq 'f') { 'Female' } else { 'Male' }
$pick = $culture | Where-Object { $_.Gender.ToString() -eq $gender } | Select-Object -First 1
if (-not $pick) { $pick = $culture | Select-Object -First 1 }
if (-not $pick) { $pick = $voices | Where-Object { $_.Gender.ToString() -eq $gender } | Select-Object -First 1 }
if ($pick) { $synth.SelectVoice($pick.Name) }
$rate = 0
if ($args.Count -ge 3) { [void][int]::TryParse($args[2], [ref]$rate) }
$synth.Rate = [Math]::Max(-10, [Math]::Min(10, $rate))
$ssml = Get-Content -LiteralPath $args[1] -Raw -Encoding UTF8
try {
    $synth.SpeakSsml($ssml)
} catch {
    $plain = [regex]::Replace($ssml, '<[^>]+>', ' ')
    $synth.Speak($plain)
}
$synth.Dispose()
"#;

pub fn choose_import() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let script = temp_path("ps1");
        if std::fs::write(&script, FILE_DIALOG_SCRIPT).is_err() {
            return None;
        }
        let output = Command::new("powershell")
            .arg("-NoProfile")
            .arg("-STA")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&script)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();
        let _ = std::fs::remove_file(&script);
        let Ok(output) = output else {
            return None;
        };
        let text = String::from_utf8_lossy(&output.stdout);
        let path = PathBuf::from(text.trim());
        path.is_file().then_some(path)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

const FILE_DIALOG_SCRIPT: &str = r#"Add-Type -AssemblyName System.Windows.Forms
$dialog = New-Object System.Windows.Forms.OpenFileDialog
$dialog.Filter = 'Texto, PDF e audio|*.txt;*.md;*.markdown;*.csv;*.json;*.pdf;*.wav;*.mp3;*.flac;*.ogg;*.opus;*.m4a;*.aac|Todos|*.*'
if ($dialog.ShowDialog() -eq 'OK') { Write-Output $dialog.FileName }
"#;

pub struct Recorder {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<Result<Vec<u8>, String>>>,
}

impl Recorder {
    pub fn start() -> Result<Self, String> {
        #[cfg(windows)]
        {
            let stop = Arc::new(AtomicBool::new(false));
            let flag = Arc::clone(&stop);
            let thread = thread::spawn(move || record_windows(flag));
            Ok(Self {
                stop,
                thread: Some(thread),
            })
        }
        #[cfg(not(windows))]
        {
            Err("o microfone está disponível no Windows".into())
        }
    }

    pub fn finish(mut self) -> Result<Vec<u8>, String> {
        self.stop.store(true, Ordering::Relaxed);
        match self.thread.take() {
            Some(handle) => handle
                .join()
                .unwrap_or_else(|_| Err("gravação interrompida".into())),
            None => Err("gravação interrompida".into()),
        }
    }
}

pub fn temp_path(extension: &str) -> PathBuf {
    static COUNT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "mydigitalbuddy-{}-{}.{}",
        std::process::id(),
        COUNT.fetch_add(1, Ordering::Relaxed),
        extension
    ))
}

pub fn write_pcm_wav(path: &Path, pcm: &[u8]) -> Result<(), String> {
    write_wav_bytes(path, pcm, 16_000)
}

pub fn write_wav_bytes(path: &Path, pcm: &[u8], rate: u32) -> Result<(), String> {
    if !pcm.len().is_multiple_of(2) || rate == 0 {
        return Err("áudio inválido".into());
    }
    let mut data = Vec::with_capacity(44 + pcm.len());
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&((36 + pcm.len()) as u32).to_le_bytes());
    data.extend_from_slice(b"WAVEfmt ");
    data.extend_from_slice(&16u32.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&rate.to_le_bytes());
    data.extend_from_slice(&(rate * 2).to_le_bytes());
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&16u16.to_le_bytes());
    data.extend_from_slice(b"data");
    data.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    data.extend_from_slice(pcm);
    std::fs::write(path, data).map_err(|_| "não foi possível gravar o áudio".to_string())
}

fn utf8_bom(text: &str) -> Vec<u8> {
    let mut data = vec![0xEF, 0xBB, 0xBF];
    data.extend_from_slice(text.as_bytes());
    data
}

static PLAYING: Mutex<Vec<u8>> = Mutex::new(Vec::new());

fn play_memory(bytes: &[u8]) -> Result<(), String> {
    #[cfg(windows)]
    {
        if bytes.len() < 44 {
            return Err("não foi possível tocar o áudio".into());
        }
        let mut held = PLAYING.lock().unwrap_or_else(|error| error.into_inner());
        stop_playback();
        held.clear();
        held.extend_from_slice(bytes);
        let flags = windows_sys::Win32::Media::Audio::SND_MEMORY
            | windows_sys::Win32::Media::Audio::SND_ASYNC
            | windows_sys::Win32::Media::Audio::SND_NODEFAULT;
        let ok = unsafe {
            windows_sys::Win32::Media::Audio::PlaySoundW(
                held.as_ptr().cast(),
                std::ptr::null_mut(),
                flags,
            )
        };
        if ok != 0 {
            Ok(())
        } else {
            Err("não foi possível tocar o áudio".into())
        }
    }
    #[cfg(not(windows))]
    {
        let _ = bytes;
        Err("não foi possível tocar o áudio".into())
    }
}

pub fn stop_playback() {
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::Media::Audio::PlaySoundW(
            std::ptr::null(),
            std::ptr::null_mut(),
            windows_sys::Win32::Media::Audio::SND_PURGE,
        );
    }
}

fn play_wav(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        let bytes =
            std::fs::read(path).map_err(|_| "não foi possível tocar o áudio".to_string())?;
        play_memory(&bytes)
    }
    #[cfg(target_os = "macos")]
    {
        return play_command("afplay", path);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        play_command("aplay", path).or_else(|_| play_command("paplay", path))
    }
}

#[cfg(unix)]
fn play_command(program: &str, path: &Path) -> Result<(), String> {
    let mut command = Command::new(program);
    command
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide_console(&mut command);
    let mut child = command
        .spawn()
        .map_err(|_| "não foi possível tocar o áudio".to_string())?;
    child
        .wait()
        .map_err(|_| "não foi possível tocar o áudio".to_string())?;
    Ok(())
}

#[cfg(windows)]
fn record_windows(stop: Arc<AtomicBool>) -> Result<Vec<u8>, String> {
    use std::mem::size_of;
    use windows_sys::Win32::Media::Audio::{
        CALLBACK_FUNCTION, HWAVEIN, WAVE_FORMAT_PCM, WAVE_MAPPER, WAVEFORMATEX, WAVEHDR,
        waveInAddBuffer, waveInClose, waveInOpen, waveInPrepareHeader, waveInReset, waveInStart,
        waveInStop, waveInUnprepareHeader,
    };
    use windows_sys::Win32::Media::MM_WIM_DATA;

    const MAX_PCM: usize = 16_000 * 2 * 60;

    struct Shared {
        samples: Mutex<Vec<u8>>,
        stop: Arc<AtomicBool>,
    }

    unsafe extern "system" fn callback(
        handle: HWAVEIN,
        message: u32,
        instance: usize,
        header: usize,
        _reserved: usize,
    ) {
        if message != MM_WIM_DATA || instance == 0 || header == 0 {
            return;
        }
        let shared = unsafe { &*(instance as *const Shared) };
        let hdr = header as *mut WAVEHDR;
        let recorded = unsafe { std::ptr::addr_of!((*hdr).dwBytesRecorded).read_unaligned() };
        let data = unsafe { std::ptr::addr_of!((*hdr).lpData).read_unaligned() };
        if recorded > 0 && !data.is_null() {
            let bytes = unsafe { std::slice::from_raw_parts(data, recorded as usize) };
            let fits = shared
                .samples
                .lock()
                .map(|samples| samples.len() + bytes.len() <= MAX_PCM)
                .unwrap_or(false);
            if fits {
                if let Ok(mut samples) = shared.samples.lock() {
                    samples.extend_from_slice(bytes);
                }
            } else {
                shared.stop.store(true, Ordering::Relaxed);
            }
        }
        if !shared.stop.load(Ordering::Relaxed) {
            unsafe {
                waveInAddBuffer(handle, hdr, size_of::<WAVEHDR>() as u32);
            }
        }
    }

    let shared = Shared {
        samples: Mutex::new(Vec::new()),
        stop: Arc::clone(&stop),
    };
    let mut handle: HWAVEIN = std::ptr::null_mut();
    let format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM as u16,
        nChannels: 1,
        nSamplesPerSec: 16_000,
        nAvgBytesPerSec: 32_000,
        nBlockAlign: 2,
        wBitsPerSample: 16,
        cbSize: 0,
    };
    let callback_ptr = callback as *const ();
    let opened = unsafe {
        waveInOpen(
            &mut handle,
            WAVE_MAPPER,
            &format,
            callback_ptr as usize,
            &shared as *const Shared as usize,
            CALLBACK_FUNCTION,
        )
    };
    if opened != 0 {
        return Err("não foi possível abrir o microfone".into());
    }
    let mut buffers: Vec<Vec<u8>> = (0..4).map(|_| vec![0u8; 8192]).collect();
    let mut headers: Vec<WAVEHDR> = buffers
        .iter_mut()
        .map(|buffer| WAVEHDR {
            lpData: buffer.as_mut_ptr(),
            dwBufferLength: buffer.len() as u32,
            dwBytesRecorded: 0,
            dwUser: 0,
            dwFlags: 0,
            dwLoops: 0,
            lpNext: std::ptr::null_mut(),
            reserved: 0,
        })
        .collect();
    for header in &mut headers {
        unsafe {
            waveInPrepareHeader(handle, header, size_of::<WAVEHDR>() as u32);
            waveInAddBuffer(handle, header, size_of::<WAVEHDR>() as u32);
        }
    }
    unsafe {
        waveInStart(handle);
    }
    let started = Instant::now();
    while !stop.load(Ordering::Relaxed) && started.elapsed() < Duration::from_secs(60) {
        thread::sleep(Duration::from_millis(40));
    }
    unsafe {
        waveInStop(handle);
        waveInReset(handle);
    }
    thread::sleep(Duration::from_millis(30));
    for header in &mut headers {
        unsafe {
            waveInUnprepareHeader(handle, header, size_of::<WAVEHDR>() as u32);
        }
    }
    unsafe {
        waveInClose(handle);
    }
    let samples = shared
        .samples
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    if samples.len() < 3_200 {
        Err("não ouvi nada".into())
    } else {
        Ok(samples)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn faster_playback_raises_the_wav_rate() {
        let path = temp_path("wav");
        write_wav_bytes(&path, &[0, 0, 0, 0], 22_050).unwrap();
        scale_wav_rate(&path, 1.2).unwrap();
        let data = std::fs::read(&path).unwrap();
        let pos = data.windows(4).position(|mark| mark == b"fmt ").unwrap();
        let rate = u32::from_le_bytes(data[pos + 12..pos + 16].try_into().unwrap());
        assert_eq!(rate, 26_460);
        let _ = std::fs::remove_file(&path);
    }
}
