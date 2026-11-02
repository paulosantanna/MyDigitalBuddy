// SPDX-License-Identifier: GPL-3.0-or-later
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_JSON: u64 = 8 * 1024 * 1024;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);
pub fn wall_time() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}
pub fn default_data_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("MYDIGITALBUDDY_DATA_DIR") {
        return p.into();
    }
    #[cfg(target_os = "windows")]
    if let Some(p) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(p).join("MyDigitalBuddy");
    }
    #[cfg(target_os = "macos")]
    if let Some(p) = std::env::var_os("HOME") {
        return PathBuf::from(p).join("Library/Application Support/MyDigitalBuddy");
    }
    if let Some(p) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(p).join("mydigitalbuddy");
    }
    if let Some(p) = std::env::var_os("HOME") {
        return PathBuf::from(p).join(".local/share/mydigitalbuddy");
    }
    PathBuf::from("memory")
}
pub fn read_json<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    let mut f = File::open(path)?;
    if f.metadata()?.len() > MAX_JSON {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "state file exceeds 8 MiB",
        ));
    }
    let mut data = String::new();
    f.read_to_string(&mut data)?;
    serde_json::from_str(&data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
pub fn atomic_bytes(path: &Path, data: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing filename"))?
        .to_string_lossy();
    let temp = parent.join(format!(
        ".{name}.{}-{}.tmp",
        std::process::id(),
        TEMP_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut f = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        f.write_all(data)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let data = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    atomic_bytes(path, &data)
}
pub fn read_episodes(path: &Path) -> io::Result<Vec<crate::core::memory::Episode>> {
    use std::io::{BufRead, BufReader};
    let f = File::open(path)?;
    if f.metadata()?.len() > MAX_JSON {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "episode log exceeds 8 MiB",
        ));
    }
    let mut out = std::collections::VecDeque::new();
    for line in BufReader::new(f).lines() {
        if let Ok(e) = serde_json::from_str::<crate::core::memory::Episode>(&line?)
            && e.valid()
        {
            out.push_back(e);
            if out.len() > 500 {
                out.pop_front();
            }
        }
    }
    Ok(out.into())
}
pub fn write_episodes(path: &Path, episodes: &[crate::core::memory::Episode]) -> io::Result<()> {
    let mut data = Vec::new();
    for ep in episodes {
        serde_json::to_writer(&mut data, ep).map_err(io::Error::other)?;
        data.push(b'\n');
    }
    atomic_bytes(path, &data)
}

/// OS-held advisory lock; automatically released on process exit or crash.
pub struct InstanceLock {
    file: File,
}
impl InstanceLock {
    pub fn acquire(root: &Path) -> io::Result<Self> {
        fs::create_dir_all(root)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("instance.lock"))?;
        file.try_lock()
            .map_err(|e| io::Error::other(format!("data directory already in use: {e}")))?;
        Ok(Self { file })
    }
}
impl Drop for InstanceLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

/// True when another live process already holds the data directory.
pub fn directory_in_use(error: &io::Error) -> bool {
    error.to_string().contains("data directory already in use")
}

pub fn log(root: &Path, message: &str) {
    let path = root.join("logs/mydigitalbuddy.log");
    if let Some(p) = path.parent() {
        let _ = fs::create_dir_all(p);
    }
    if fs::metadata(&path).is_ok_and(|m| m.len() > 1_000_000) {
        let _ = fs::rename(&path, root.join("logs/mydigitalbuddy.previous.log"));
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{:.3} {message}", wall_time());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_in_use_matches_the_lock_error() {
        let locked = io::Error::other("data directory already in use: busy");
        let other = io::Error::other("permission denied");
        assert!(directory_in_use(&locked));
        assert!(!directory_in_use(&other));
    }
}
