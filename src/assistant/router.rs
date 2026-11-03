// SPDX-License-Identifier: GPL-3.0-or-later
//! Local llama.cpp on 127.0.0.1, or several NVIDIA models when that permission is on.
//! The first model to answer is kept, and the others are told to stop.
//! API keys are read for the request and are never written to logs or the chat.

use super::media::find_file;
use crate::{
    config::Settings,
    dialogue::{self, PromptMessage, local_origin, redact},
};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

const NVIDIA_URL: &str = "https://integrate.api.nvidia.com/v1/chat/completions";
const NVIDIA_WAIT: Duration = Duration::from_secs(12);
/// Faster models are listed first. All of them run together; the first text wins.
const NVIDIA_MODELS: &[&str] = &[
    "nvidia/nemotron-3.5-lightning-30b-a3b",
    "nvidia/nemotron-3-nano-30b-a3b",
    "nvidia/nemotron-3.5-bolt-e4b",
    "nvidia/nemotron-3.5-super-vl-120b-a12b",
    "nvidia/nemotron-3-super-120b-a12b",
    "nvidia/nemotron-3-ultra-550b-a55b",
    "deepseek-ai/deepseek-v4.1-flash",
];

pub struct LocalProcess {
    pub child: Child,
    pub port: u16,
    pub model: PathBuf,
    pub stderr: Arc<Mutex<String>>,
}

pub fn complete(
    messages: &[PromptMessage],
    settings: &Settings,
    content_root: &Path,
    data_root: &Path,
    llama: &Mutex<Option<LocalProcess>>,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<String, String> {
    if settings.cloud_deepseek {
        nvidia(messages, settings, cancel)
    } else if settings.assistant {
        local(messages, settings, content_root, data_root, llama, cancel)
    } else {
        Err("ligue o assistente local ou a NVIDIA, em Permissões".into())
    }
}

pub fn stop_local(llama: &Mutex<Option<LocalProcess>>) {
    if let Some(mut process) = llama
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take()
    {
        let _ = process.child.kill();
        let _ = process.child.wait();
    }
}

pub fn load_nvapi_keys(settings: &Settings) -> Result<Vec<String>, String> {
    if let Ok(env) = std::env::var("NVIDIA_API_KEY") {
        let keys = dialogue::parse_nvapi_keys(&env);
        if !keys.is_empty() {
            return Ok(keys);
        }
        if env.trim().starts_with("nvapi-") {
            return Err("chave NVIDIA do ambiente foi recusada".into());
        }
    }
    let path = key_path(settings);
    if path.as_os_str().is_empty() {
        return Err("arquivo de chave não configurado".into());
    }
    let meta = std::fs::metadata(&path).map_err(|_| "arquivo de chave inacessível".to_string())?;
    if meta.len() > 65_536 {
        return Err("arquivo de chave grande demais".into());
    }
    let text =
        std::fs::read_to_string(&path).map_err(|_| "arquivo de chave inacessível".to_string())?;
    let keys = dialogue::parse_nvapi_keys(&text);
    if keys.is_empty() {
        Err("nenhuma linha nvapi- nesse arquivo".into())
    } else {
        Ok(keys)
    }
}

fn key_path(settings: &Settings) -> PathBuf {
    if let Ok(path) = std::env::var("MYDIGITALBUDDY_NVIDIA_KEY_FILE")
        && !path.trim().is_empty()
    {
        return PathBuf::from(path.trim());
    }
    PathBuf::from(settings.nvidia_key_file.trim())
}

enum Attempt {
    Reply(String),
    Auth,
    Failed(String),
}

fn nvidia(
    messages: &[PromptMessage],
    settings: &Settings,
    external: &AtomicBool,
) -> Result<String, String> {
    let keys = load_nvapi_keys(settings)?;
    let mut last = "a NVIDIA recusou a autenticação".to_string();
    for key in &keys {
        if external.load(Ordering::Relaxed) {
            return Err("interrompido".into());
        }
        match race_key(key, messages, external) {
            Ok(text) => return Ok(text),
            Err(message) if message == "interrompido" => return Err(message),
            Err(message) => last = message,
        }
    }
    Err(last)
}

fn race_key(
    key: &str,
    messages: &[PromptMessage],
    external: &AtomicBool,
) -> Result<String, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    for model in NVIDIA_MODELS {
        let tx = tx.clone();
        let stop = Arc::clone(&stop);
        let key = key.to_string();
        let messages = messages.to_vec();
        let model = *model;
        thread::spawn(move || {
            if stop.load(Ordering::Relaxed) {
                let _ = tx.send(Attempt::Failed("encerrado".into()));
                return;
            }
            let _ = tx.send(ask_model(&key, model, &messages));
        });
    }
    drop(tx);
    finish_race(rx, NVIDIA_MODELS.len(), &stop, external)
}

fn finish_race(
    rx: mpsc::Receiver<Attempt>,
    mut pending: usize,
    stop: &AtomicBool,
    external: &AtomicBool,
) -> Result<String, String> {
    let mut failures = Vec::new();
    let mut auths = 0usize;
    while pending > 0 {
        if external.load(Ordering::Relaxed) {
            stop.store(true, Ordering::Relaxed);
            return Err("interrompido".into());
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Attempt::Reply(text)) => {
                stop.store(true, Ordering::Relaxed);
                return Ok(text);
            }
            Ok(Attempt::Auth) => {
                pending -= 1;
                auths += 1;
            }
            Ok(Attempt::Failed(message)) => {
                pending -= 1;
                if message != "encerrado" {
                    failures.push(message);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    if auths > 0 && failures.is_empty() {
        Err("a NVIDIA recusou a autenticação".into())
    } else if let Some(message) = failures.first() {
        Err(redact(message))
    } else {
        Err("nenhum modelo da NVIDIA respondeu".into())
    }
}

fn ask_model(key: &str, model: &str, messages: &[PromptMessage]) -> Attempt {
    let body = chat_body(model, messages, 1.0, 0.95, Some(false));
    match post_nvidia(key, &body) {
        Ok((200, text)) => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(value) => match dialogue::extract_reply(&value) {
                Ok(reply) => Attempt::Reply(reply),
                Err(error) => Attempt::Failed(format!("{model}: {error}")),
            },
            Err(_) => Attempt::Failed(format!("{model}: resposta NVIDIA inválida")),
        },
        Ok((401 | 403, _)) => Attempt::Auth,
        Ok((status, text)) => Attempt::Failed(format!(
            "{model} respondeu {status}: {}",
            redact(&json_message(&text))
        )),
        Err(error) => Attempt::Failed(format!("{model}: {}", redact(&error))),
    }
}

fn post_nvidia(key: &str, body: &serde_json::Value) -> Result<(u16, String), String> {
    let auth = format!("Bearer {key}");
    let agent = nvidia_agent();
    let sent = agent
        .post(NVIDIA_URL)
        .header("Authorization", &auth)
        .header("Accept", "application/json")
        .send_json(body);
    drop(auth);
    match sent {
        Ok(mut response) => {
            let status = response.status().as_u16();
            let text = response
                .body_mut()
                .read_to_string()
                .map_err(|_| "não foi possível ler a resposta da NVIDIA".to_string())?;
            if text.len() > 1_000_000 {
                return Err("resposta grande demais".into());
            }
            Ok((status, text))
        }
        Err(ureq::Error::StatusCode(status)) => Ok((status, String::new())),
        Err(error) => Err(redact(&error.to_string())),
    }
}

fn nvidia_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(NVIDIA_WAIT))
        .build()
        .into()
}

fn local(
    messages: &[PromptMessage],
    settings: &Settings,
    content_root: &Path,
    data_root: &Path,
    llama: &Mutex<Option<LocalProcess>>,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<String, String> {
    let port = ensure_local(settings, content_root, data_root, llama, cancel)?;
    let origin = local_origin(port)?;
    let body = serde_json::to_vec(&chat_body("qwen", messages, 0.7, 0.9, None))
        .map_err(|_| "não foi possível montar o pedido".to_string())?;
    let (status, bytes) = post_local(&origin, &body)?;
    if status != 200 {
        let snippet = String::from_utf8_lossy(&bytes);
        return Err(format!(
            "o modelo local respondeu {status}: {}",
            redact(&snippet)
        ));
    }
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "resposta local inválida".to_string())?;
    dialogue::extract_reply(&value)
}

fn ensure_local(
    settings: &Settings,
    content_root: &Path,
    data_root: &Path,
    llama: &Mutex<Option<LocalProcess>>,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<u16, String> {
    let roots = super::roots(content_root, data_root);
    let model = find_file(
        &settings.model_path,
        &["qwen2.5-7b-instruct-q4_k_m.gguf", "qwen.gguf"],
        &roots,
        Some("gguf"),
    )
    .ok_or_else(|| {
        "não encontrei o GGUF. O recomendado nesta placa é Qwen2.5-7B-Instruct Q4_K_M, em models/qwen.gguf ao lado do programa".to_string()
    })?;
    if let Some(port) = reusable_port(llama, &model)? {
        return Ok(port);
    }
    stop_local(llama);
    let server = find_file(
        &settings.llama_server,
        &["llama-server", "llama-server.exe"],
        &roots,
        None,
    )
    .ok_or_else(|| {
        "não encontrei o llama-server. Coloque o executável do llama.cpp ao lado do programa. Ollama não é usado".to_string()
    })?;
    let port = free_port()?;
    let process = spawn_server(&server, &model, port)?;
    {
        *llama.lock().unwrap_or_else(|error| error.into_inner()) = Some(process);
    }
    wait_ready(port, llama, cancel)?;
    Ok(port)
}

fn reusable_port(llama: &Mutex<Option<LocalProcess>>, model: &Path) -> Result<Option<u16>, String> {
    let mut guard = llama.lock().unwrap_or_else(|error| error.into_inner());
    let Some(process) = guard.as_mut() else {
        return Ok(None);
    };
    if process.model != model {
        return Ok(None);
    }
    match process.child.try_wait() {
        Ok(None) => Ok(Some(process.port)),
        Ok(Some(_)) => Ok(None),
        Err(error) => Err(format!(
            "não foi possível consultar o modelo local: {error}"
        )),
    }
}

fn spawn_server(server: &Path, model: &Path, port: u16) -> Result<LocalProcess, String> {
    let mut command = Command::new(server);
    command
        .arg("-m")
        .arg(model)
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .arg("-c")
        .arg("4096")
        .arg("-b")
        .arg("256")
        .arg("-ub")
        .arg("128")
        .arg("-ngl")
        .arg("99")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    hide_console(&mut command);
    let mut child = command.spawn().map_err(|_| {
        "não foi possível abrir o llama-server. Confira se o executável é o do llama.cpp"
            .to_string()
    })?;
    let stderr = Arc::new(Mutex::new(String::new()));
    if let Some(pipe) = child.stderr.take() {
        let log = Arc::clone(&stderr);
        thread::spawn(move || drain_stderr(pipe, log));
    }
    Ok(LocalProcess {
        child,
        port,
        model: model.to_path_buf(),
        stderr,
    })
}

fn drain_stderr(pipe: impl Read, log: Arc<Mutex<String>>) {
    let mut reader = std::io::BufReader::new(pipe);
    let mut line = String::new();
    loop {
        line.clear();
        match std::io::BufRead::read_line(&mut reader, &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let mut guard = log.lock().unwrap_or_else(|error| error.into_inner());
                guard.push_str(&line);
                if guard.len() > 4000 {
                    let drain = guard.len() - 2000;
                    guard.drain(..drain);
                }
            }
        }
    }
}

fn wait_ready(
    port: u16,
    llama: &Mutex<Option<LocalProcess>>,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<(), String> {
    for _ in 0..120 {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("interrompido".into());
        }
        if let Some(error) = process_exit(llama) {
            return Err(error);
        }
        if http_ok(port, "/health") || http_ok(port, "/v1/models") {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(500));
    }
    Err("o modelo local não ficou pronto a tempo".into())
}

fn process_exit(llama: &Mutex<Option<LocalProcess>>) -> Option<String> {
    let mut guard = llama.lock().unwrap_or_else(|error| error.into_inner());
    let process = guard.as_mut()?;
    match process.child.try_wait() {
        Ok(Some(status)) => {
            let tail = process
                .stderr
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone();
            Some(format!(
                "o modelo local encerrou ({status}). {}",
                redact(&tail)
            ))
        }
        _ => None,
    }
}

fn free_port() -> Result<u16, String> {
    for port in 18_181..18_191 {
        let address = SocketAddr::from(([127, 0, 0, 1], port));
        if TcpStream::connect_timeout(&address, Duration::from_millis(40)).is_err() {
            return Ok(port);
        }
    }
    Err("não há porta local livre para o modelo".into())
}

fn http_ok(port: u16, path: &str) -> bool {
    let Ok(mut stream) = TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(200),
    ) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(400)));
    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut buffer = [0u8; 64];
    let Ok(size) = stream.read(&mut buffer) else {
        return false;
    };
    String::from_utf8_lossy(&buffer[..size]).contains(" 200 ")
}

fn post_local(origin: &str, body: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let port: u16 = origin
        .rsplit(':')
        .next()
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| "porta local inválida".to_string())?;
    if !origin.starts_with("http://127.0.0.1:") {
        return Err("o modelo local só fala em 127.0.0.1".into());
    }
    let mut stream = TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_secs(3),
    )
    .map_err(|_| "o modelo local não aceitou conexão".to_string())?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(120)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let header = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .and_then(|_| stream.write_all(body))
        .map_err(|_| "não foi possível enviar o pedido local".to_string())?;
    let bytes = read_limited(&mut stream, 1_000_000)?;
    let (status, payload) = split_http(&bytes)?;
    Ok((status, payload))
}

fn read_limited(stream: &mut TcpStream, max: usize) -> Result<Vec<u8>, String> {
    let mut buffer = [0u8; 8192];
    let mut out = Vec::new();
    loop {
        let size = stream
            .read(&mut buffer)
            .map_err(|_| "leitura local interrompida".to_string())?;
        if size == 0 {
            break;
        }
        if out.len() + size > max {
            return Err("resposta grande demais".into());
        }
        out.extend_from_slice(&buffer[..size]);
    }
    Ok(out)
}

fn split_http(bytes: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or("resposta local inválida")?;
    let header = String::from_utf8_lossy(&bytes[..split]);
    let status = header
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    let raw = &bytes[split + 4..];
    let body = if header
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        decode_chunked(raw)?
    } else {
        raw.to_vec()
    };
    Ok((status, body))
}

fn decode_chunked(mut body: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = body
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or("chunk inválido")?;
        let line = std::str::from_utf8(&body[..line_end]).map_err(|_| "chunk inválido")?;
        let size = usize::from_str_radix(line.trim().split(';').next().unwrap_or(""), 16)
            .map_err(|_| "chunk inválido")?;
        body = &body[line_end + 2..];
        if size == 0 {
            break;
        }
        if body.len() < size + 2 {
            return Err("chunk inválido".into());
        }
        out.extend_from_slice(&body[..size]);
        body = &body[size + 2..];
        if out.len() > 1_000_000 {
            return Err("resposta grande demais".into());
        }
    }
    Ok(out)
}

fn chat_body(
    model: &str,
    messages: &[PromptMessage],
    temperature: f64,
    top_p: f64,
    thinking: Option<bool>,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": model,
        "messages": messages.iter().map(|message| serde_json::json!({
            "role": message.role,
            "content": message.content,
        })).collect::<Vec<_>>(),
        "max_tokens": 800,
        "temperature": temperature,
        "top_p": top_p,
        "stream": false,
    });
    if let Some(thinking) = thinking {
        body["chat_template_kwargs"] = serde_json::json!({
            "thinking": thinking,
            "enable_thinking": thinking,
        });
    }
    body
}

fn json_message(text: &str) -> String {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .or_else(|| value.pointer("/message"))
                .and_then(|item| item.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| text.chars().take(180).collect())
}

pub fn hide_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn local_http_body_is_split_without_touching_the_network() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\n{\"ok\":true}";
        let (status, body) = split_http(raw).unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, b"{\"ok\":true}");
        let chunked =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let (_, body) = split_http(chunked).unwrap();
        assert_eq!(body, b"hello");
    }

    #[test]
    fn nvidia_request_turns_thinking_off() {
        let messages = [PromptMessage {
            role: "user",
            content: "oi".into(),
        }];
        let body = chat_body(
            "deepseek-ai/deepseek-v4.1-flash",
            &messages,
            1.0,
            0.95,
            Some(false),
        );
        assert_eq!(body["chat_template_kwargs"]["thinking"], false);
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(body["stream"], false);
        let local = chat_body("qwen", &messages, 0.7, 0.9, None);
        assert!(local.get("chat_template_kwargs").is_none());
    }

    #[test]
    fn nvidia_roster_includes_super_ultra_and_deepseek() {
        let ids = NVIDIA_MODELS.join(" ");
        assert!(ids.contains("nemotron-3.5-super"));
        assert!(ids.contains("nemotron-3-ultra"));
        assert!(ids.contains("deepseek-ai/deepseek-v4.1-flash"));
        assert!(NVIDIA_MODELS.len() >= 4);
    }

    #[test]
    fn first_nvidia_reply_ends_the_other_attempts() {
        let stop = AtomicBool::new(false);
        let external = AtomicBool::new(false);
        let (tx, rx) = mpsc::channel();
        let late = tx.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(80));
            let _ = late.send(Attempt::Reply("primeiro".into()));
        });
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(2));
            let _ = tx.send(Attempt::Reply("tarde".into()));
        });
        let started = std::time::Instant::now();
        let reply = finish_race(rx, 2, &stop, &external).unwrap();
        assert_eq!(reply, "primeiro");
        assert!(started.elapsed() < Duration::from_millis(800));
        assert!(stop.load(Ordering::Relaxed));
    }
}
