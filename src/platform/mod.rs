// SPDX-License-Identifier: GPL-3.0-or-later
use crate::core::physics::{Platform, Rect};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

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
