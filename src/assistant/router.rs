// SPDX-License-Identifier: GPL-3.0-or-later
//! Local llama.cpp on 127.0.0.1, or DeepSeek 4.1 on NVIDIA when that permission is on.
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
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

const NVIDIA_URL: &str = "https://integrate.api.nvidia.com/v1/chat/completions";
const NVIDIA_MODEL: &str = "deepseek-ai/deepseek-v4.1-flash";

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
        nvidia(messages, settings)
    } else if settings.assistant {
        local(messages, settings, content_root, data_root, llama, cancel)
    } else {
        Err("ligue o assistente local ou o DeepSeek na NVIDIA, em Permissões".into())
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

fn nvidia(messages: &[PromptMessage], settings: &Settings) -> Result<String, String> {
    let keys = load_nvapi_keys(settings)?;
    let body = chat_body(NVIDIA_MODEL, messages, 1.0, 0.95);
    let mut last = "a NVIDIA recusou a autenticação".to_string();
    for key in &keys {
        match post_nvidia(key, &body) {
            Ok((200, text)) => {
                let value = serde_json::from_str(&text).map_err(|_| "resposta NVIDIA inválida")?;
                return dialogue::extract_reply(&value);
            }
            Ok((401 | 403, _)) => {
                last = "a NVIDIA recusou a autenticação".into();
            }
            Ok((status, text)) => {
                last = format!(
                    "a NVIDIA respondeu {status}: {}",
                    redact(&json_message(&text))
                );
            }
            Err(error) => last = redact(&error),
        }
    }
    Err(last)
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
        .timeout_global(Some(Duration::from_secs(120)))
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
    let body = serde_json::to_vec(&chat_body("qwen", messages, 0.7, 0.9))
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
) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "messages": messages.iter().map(|message| serde_json::json!({
            "role": message.role,
            "content": message.content,
        })).collect::<Vec<_>>(),
        "max_tokens": 800,
        "temperature": temperature,
        "top_p": top_p,
        "stream": false,
    })
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
}
