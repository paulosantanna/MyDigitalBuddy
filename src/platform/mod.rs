// SPDX-License-Identifier: GPL-3.0-or-later
use crate::core::physics::{Platform, Rect};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
mod windows_session;

#[derive(Clone, Debug)]
pub struct NativeWindow {
    pub id: u64,
    pub rect: Rect,
    pub maximized: bool,
}
#[derive(Clone, Default, Debug)]
pub struct Snapshot {
    pub app: String,
    pub idle_seconds: Option<f64>,
    pub cursor: (f64, f64),
    pub windows: Vec<NativeWindow>,
    pub work_area: Option<Rect>,
    pub fullscreen: bool,
    pub panic: bool,
    pub status: String,
}

/// Shell windows cover the monitor without being a game or a video.
/// Pestinha ignores them; treating them as fullscreen hid the pet on a desktop click.
pub fn is_shell_class(class: &str) -> bool {
    matches!(
        class,
        "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
    )
}

/// Asks the instance that already owns `root` to show its window.
/// The running desktop listens; a second launch uses this instead of exiting silently.
pub fn request_running_instance(root: &std::path::Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        windows_session::request_running_instance(root)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = root;
        false
    }
}

/// Shown when a second launch finds the data directory locked and nobody answers.
pub fn notify_running_instance_unresponsive() {
    #[cfg(target_os = "windows")]
    windows_session::notify_running_instance_unresponsive();
}

/// Stable kernel-object name for the per-directory show signal.
pub(crate) fn activation_object_name(root: &std::path::Path) -> String {
    let path = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let text = path.to_string_lossy().into_owned();
    #[cfg(target_os = "windows")]
    let text = text.replace('/', "\\").to_lowercase();
    format!(
        "Local\\MyDigitalBuddy.Show.{:016x}",
        fnv1a64(text.as_bytes())
    )
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(target_os = "windows")]
pub use windows_session::Activation;

#[cfg(not(target_os = "windows"))]
pub struct Activation;

#[cfg(not(target_os = "windows"))]
impl Activation {
    pub fn listen(_root: &std::path::Path) -> Option<Self> {
        None
    }

    pub fn poll(&self) -> bool {
        false
    }
}

pub fn startup_command(path: &str) -> Result<String, String> {
    let path = path.strip_prefix(r"\\?\").unwrap_or(path);
    if path.is_empty() || path.contains('"') {
        return Err("caminho do executável inválido".into());
    }
    Ok(format!("\"{path}\" --tray"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayCommand {
    Open,
    TogglePause,
    ToggleHidden,
    Panic,
    Quit,
}

#[cfg(target_os = "windows")]
pub use windows_session::Tray;

#[cfg(not(target_os = "windows"))]
pub struct Tray;

#[cfg(not(target_os = "windows"))]
impl Tray {
    pub fn install() -> Result<Self, String> {
        Err("bandeja disponível no Windows".into())
    }
    pub fn poll(&mut self) -> Vec<TrayCommand> {
        Vec::new()
    }
    pub fn set_flags(&self, _paused: bool, _hidden: bool) {}
}

pub fn supports_launch_at_startup() -> bool {
    cfg!(target_os = "windows")
}

pub fn set_launch_at_startup(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        windows_session::set_launch_at_startup(enabled)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = enabled;
        Err("Iniciar com o Windows está disponível apenas no Windows.".into())
    }
}

pub fn sample() -> Snapshot {
    #[cfg(target_os = "windows")]
    {
        windows::sample()
    }
    #[cfg(target_os = "linux")]
    {
        linux::sample()
    }
    #[cfg(target_os = "macos")]
    {
        macos::sample()
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        Snapshot {
            status: "Desktop sensors unavailable on this platform".into(),
            ..Default::default()
        }
    }
}

pub fn platforms(
    windows: &[NativeWindow],
    exclude: &[u64],
    bounds: Rect,
    scale: f64,
) -> Vec<Platform> {
    let mut above: Vec<Rect> = Vec::new();
    let mut out = Vec::new();
    for w in windows {
        if exclude.contains(&w.id) {
            continue;
        }
        let r = Rect {
            left: w.rect.left / scale,
            top: w.rect.top / scale,
            right: w.rect.right / scale,
            bottom: w.rect.bottom / scale,
        };
        let mid = (r.left + r.right) / 2.0;
        let covered = above.iter().any(|a| a.contains(mid, r.top + 2.0));
        above.push(r);
        if !w.maximized
            && !covered
            && mid >= bounds.left
            && mid <= bounds.right
            && r.top >= bounds.top + 40.0
            && r.top <= bounds.bottom - 100.0
        {
            out.push(Platform {
                id: w.id,
                left: r.left + 10.0,
                right: r.right - 10.0,
                top: r.top,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn desktop_shell_is_not_a_fullscreen_app() {
        assert!(super::is_shell_class("Progman"));
        assert!(super::is_shell_class("WorkerW"));
        assert!(super::is_shell_class("Shell_TrayWnd"));
        assert!(super::is_shell_class("Shell_SecondaryTrayWnd"));
        assert!(!super::is_shell_class("Chrome_WidgetWin_1"));
    }

    #[test]
    fn activation_name_is_stable_and_path_safe() {
        let root = std::path::Path::new(r"C:\No\Such\MyDigitalBuddy-Activation");
        let name = super::activation_object_name(root);
        assert_eq!(name, super::activation_object_name(root));
        assert!(name.starts_with("Local\\MyDigitalBuddy.Show."));
        assert_eq!(name.len(), "Local\\MyDigitalBuddy.Show.".len() + 16);
        assert!(
            name.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'\\' || b == b'.')
        );
        assert_ne!(
            name,
            super::activation_object_name(std::path::Path::new(
                r"C:\No\Such\MyDigitalBuddy-Activation-Other"
            ))
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn activation_name_ignores_windows_case_and_slashes() {
        let upper = std::path::Path::new(r"C:\No\Such\MyDigitalBuddy-Case");
        let lower = std::path::Path::new(r"c:\no\such\mydigitalbuddy-case");
        let slashed = std::path::Path::new("C:/No/Such/MyDigitalBuddy-Case");
        assert_eq!(
            super::activation_object_name(upper),
            super::activation_object_name(lower)
        );
        assert_eq!(
            super::activation_object_name(upper),
            super::activation_object_name(slashed)
        );
    }

    #[test]
    fn startup_command_quotes_the_executable() {
        let cmd = super::startup_command(r"C:\Program Files\MyDigitalBuddy.exe");
        assert_eq!(
            cmd.as_deref(),
            Ok(r#""C:\Program Files\MyDigitalBuddy.exe" --tray"#)
        );
        let prefixed = super::startup_command(r"\\?\E:\Github\MyDigitalBuddy\mydigitalbuddy.exe");
        assert_eq!(
            prefixed.as_deref(),
            Ok(r#""E:\Github\MyDigitalBuddy\mydigitalbuddy.exe" --tray"#)
        );
        assert!(super::startup_command("").is_err());
    }
}
