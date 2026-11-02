// SPDX-License-Identifier: GPL-3.0-or-later
use super::{NativeWindow, Snapshot};
use crate::core::physics::Rect;
use std::ffi::c_void;

#[repr(C)]
#[derive(Default)]
struct WinRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
#[repr(C)]
#[derive(Default)]
struct Point {
    x: i32,
    y: i32,
}
#[repr(C)]
struct LastInput {
    size: u32,
    tick: u32,
}
type Hwnd = *mut c_void;

#[link(name = "user32")]
unsafe extern "system" {
    fn GetForegroundWindow() -> Hwnd;
    fn GetWindowThreadProcessId(window: Hwnd, pid: *mut u32) -> u32;
    fn GetCursorPos(point: *mut Point) -> i32;
    fn GetLastInputInfo(input: *mut LastInput) -> i32;
    fn GetAsyncKeyState(key: i32) -> i16;
    fn IsWindowVisible(window: Hwnd) -> i32;
    fn IsIconic(window: Hwnd) -> i32;
    fn IsZoomed(window: Hwnd) -> i32;
    fn EnumWindows(callback: unsafe extern "system" fn(Hwnd, isize) -> i32, param: isize) -> i32;
    fn GetWindowRect(window: Hwnd, rect: *mut WinRect) -> i32;
    fn GetClassNameW(window: Hwnd, name: *mut u16, max: i32) -> i32;
    fn SystemParametersInfoW(action: u32, param: u32, value: *mut c_void, flags: u32) -> i32;
    fn GetSystemMetrics(index: i32) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetTickCount() -> u32;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn QueryFullProcessImageNameW(
        process: *mut c_void,
        flags: u32,
        name: *mut u16,
        size: *mut u32,
    ) -> i32;
}
#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmGetWindowAttribute(window: Hwnd, attribute: u32, value: *mut c_void, size: u32) -> i32;
}

fn rect(r: WinRect) -> Rect {
    Rect {
        left: r.left as f64,
        top: r.top as f64,
        right: r.right as f64,
        bottom: r.bottom as f64,
    }
}

unsafe extern "system" fn collect(window: Hwnd, param: isize) -> i32 {
    // SAFETY: EnumWindows calls synchronously; param points to the live Vec below.
    unsafe {
        if IsWindowVisible(window) == 0 || IsIconic(window) != 0 {
            return 1;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(window, &mut pid);
        if pid == std::process::id() {
            return 1;
        }
        let mut cloaked = 0_u32;
        if DwmGetWindowAttribute(window, 14, (&mut cloaked as *mut u32).cast(), 4) == 0
            && cloaked != 0
        {
            return 1;
        }
        let mut r = WinRect::default();
        if DwmGetWindowAttribute(
            window,
            9,
            (&mut r as *mut WinRect).cast(),
            std::mem::size_of::<WinRect>() as u32,
        ) != 0
            && GetWindowRect(window, &mut r) == 0
        {
            return 1;
        }
        if r.right - r.left >= 160 && r.bottom - r.top >= 80 {
            let out = &mut *(param as *mut Vec<NativeWindow>);
            out.push(NativeWindow {
                id: window as u64,
                rect: rect(r),
                maximized: IsZoomed(window) != 0,
            });
        }
        1
    }
}

pub fn sample() -> Snapshot {
    // SAFETY: all output buffers are initialized, sized and valid throughout each call;
    // process handles are closed, and only application names/geometries are queried.
    unsafe {
        let mut out = Snapshot::default();
        let mut cursor = Point::default();
        if GetCursorPos(&mut cursor) != 0 {
            out.cursor = (cursor.x as f64, cursor.y as f64);
        }
        let mut input = LastInput {
            size: std::mem::size_of::<LastInput>() as u32,
            tick: 0,
        };
        if GetLastInputInfo(&mut input) != 0 {
            out.idle_seconds = Some(GetTickCount().wrapping_sub(input.tick) as f64 / 1000.0);
        }
        out.panic = [0x11, 0x12, 0x10, 0x42]
            .iter()
            .all(|k| GetAsyncKeyState(*k) < 0);
        let window = GetForegroundWindow();
        let mut pid = 0;
        GetWindowThreadProcessId(window, &mut pid);
        let process = OpenProcess(0x1000, 0, pid);
        if !process.is_null() {
            let mut name = vec![0_u16; 32768];
            let mut size = name.len() as u32;
            if QueryFullProcessImageNameW(process, 0, name.as_mut_ptr(), &mut size) != 0 {
                let full = String::from_utf16_lossy(&name[..size as usize]);
                out.app = full.rsplit('\\').next().unwrap_or("").to_lowercase();
            }
            CloseHandle(process);
        }
        let mut work = WinRect::default();
        if SystemParametersInfoW(0x30, 0, (&mut work as *mut WinRect).cast(), 0) != 0 {
            out.work_area = Some(rect(work));
        }
        let mut front = WinRect::default();
        if !window.is_null() && pid != std::process::id() && GetWindowRect(window, &mut front) != 0
        {
            let mut class_buf = [0_u16; 256];
            let class_len = GetClassNameW(window, class_buf.as_mut_ptr(), class_buf.len() as i32);
            let class = if class_len > 0 {
                String::from_utf16_lossy(&class_buf[..class_len as usize])
            } else {
                String::new()
            };
            let sw = GetSystemMetrics(0);
            let sh = GetSystemMetrics(1);
            // Progman/WorkerW cover the monitor and are not maximized. Counting
            // that as fullscreen hid the pet as soon as the user clicked the desktop.
            out.fullscreen = !super::is_shell_class(&class)
                && front.left <= 0
                && front.top <= 0
                && front.right >= sw
                && front.bottom >= sh
                && IsZoomed(window) == 0;
        }
        EnumWindows(
            collect,
            (&mut out.windows as *mut Vec<NativeWindow>) as isize,
        );
        out.status = "Windows: sensores locais ativos".into();
        out
    }
}
