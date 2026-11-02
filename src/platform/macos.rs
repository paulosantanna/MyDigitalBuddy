// SPDX-License-Identifier: GPL-3.0-or-later
use super::{NativeWindow, Snapshot};
use crate::core::physics::Rect;
use std::{
    ffi::{c_char, c_int, c_void},
    ptr,
};
type CF = *const c_void;
#[repr(C)]
#[derive(Default)]
struct Point {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Default)]
struct Size {
    w: f64,
    h: f64,
}
#[repr(C)]
#[derive(Default)]
struct CGRect {
    origin: Point,
    size: Size,
}
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGWindowListCopyWindowInfo(options: u32, relative: u32) -> CF;
    static kCGWindowOwnerPID: CF;
    static kCGWindowNumber: CF;
    static kCGWindowLayer: CF;
    static kCGWindowBounds: CF;
    fn CGRectMakeWithDictionaryRepresentation(dict: CF, rect: *mut CGRect) -> bool;
    fn CGMainDisplayID() -> u32;
    fn CGDisplayBounds(id: u32) -> CGRect;
    fn CGEventSourceSecondsSinceLastEventType(state: i32, event: u32) -> f64;
    fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
    fn CGEventCreate(source: CF) -> CF;
    fn CGEventGetLocation(event: CF) -> Point;
    fn CGPreflightScreenCaptureAccess() -> bool;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: CF);
    fn CFArrayGetCount(array: CF) -> isize;
    fn CFArrayGetValueAtIndex(array: CF, index: isize) -> CF;
    fn CFDictionaryGetValue(dict: CF, key: CF) -> CF;
    fn CFNumberGetValue(number: CF, kind: isize, result: *mut c_void) -> bool;
}
#[link(name = "proc")]
unsafe extern "C" {
    fn proc_name(pid: c_int, buffer: *mut c_void, size: u32) -> c_int;
}

unsafe fn number(dict: CF, key: CF) -> i32 {
    unsafe {
        let raw = CFDictionaryGetValue(dict, key);
        let mut n = 0;
        if !raw.is_null() {
            CFNumberGetValue(raw, 3, (&mut n as *mut i32).cast());
        }
        n
    }
}

pub fn sample() -> Snapshot {
    // SAFETY: CoreFoundation Copy/Create values are released and never retained past this call.
    // Window titles/content are never read. No permission prompts are triggered automatically.
    unsafe {
        let mut out = Snapshot::default();
        let screen = CGDisplayBounds(CGMainDisplayID());
        out.work_area = Some(Rect {
            left: screen.origin.x,
            top: screen.origin.y,
            right: screen.origin.x + screen.size.w,
            bottom: screen.origin.y + screen.size.h,
        });
        let event = CGEventCreate(ptr::null());
        if !event.is_null() {
            let p = CGEventGetLocation(event);
            out.cursor = (p.x, p.y);
            CFRelease(event);
        }
        let idle = CGEventSourceSecondsSinceLastEventType(1, u32::MAX);
        if idle.is_finite() {
            out.idle_seconds = Some(idle);
        }
        out.panic = CGEventSourceKeyState(1, 11)
            && (CGEventSourceKeyState(1, 59) || CGEventSourceKeyState(1, 62))
            && (CGEventSourceKeyState(1, 58) || CGEventSourceKeyState(1, 61))
            && (CGEventSourceKeyState(1, 56) || CGEventSourceKeyState(1, 60));
        let allowed = CGPreflightScreenCaptureAccess();
        let list = CGWindowListCopyWindowInfo(1 | 16, 0);
        if !list.is_null() {
            for i in 0..CFArrayGetCount(list) {
                let d = CFArrayGetValueAtIndex(list, i);
                if d.is_null() || number(d, kCGWindowLayer) != 0 {
                    continue;
                }
                let pid = number(d, kCGWindowOwnerPID);
                if pid == std::process::id() as i32 {
                    continue;
                }
                if out.app.is_empty() {
                    let mut buf = [0 as c_char; 1024];
                    if proc_name(pid, buf.as_mut_ptr().cast(), 1024) > 0 {
                        out.app = std::ffi::CStr::from_ptr(buf.as_ptr())
                            .to_string_lossy()
                            .to_lowercase();
                    }
                }
                let raw = CFDictionaryGetValue(d, kCGWindowBounds);
                let mut r = CGRect::default();
                if !raw.is_null()
                    && CGRectMakeWithDictionaryRepresentation(raw, &mut r)
                    && r.size.w >= 160.0
                    && r.size.h >= 80.0
                {
                    let rect = Rect {
                        left: r.origin.x,
                        top: r.origin.y,
                        right: r.origin.x + r.size.w,
                        bottom: r.origin.y + r.size.h,
                    };
                    if out.windows.is_empty() {
                        out.fullscreen = rect.left <= screen.origin.x
                            && rect.top <= screen.origin.y
                            && rect.right >= screen.origin.x + screen.size.w
                            && rect.bottom >= screen.origin.y + screen.size.h;
                    }
                    out.windows.push(NativeWindow {
                        id: number(d, kCGWindowNumber) as u64,
                        rect,
                        maximized: false,
                    });
                }
            }
            CFRelease(list);
        }
        out.status=if allowed {"macOS: metadados locais; atalho depende da permissão de entrada"} else {"macOS: metadados de janelas limitados pelo sistema; habilite a permissão nas configurações do macOS se desejar"}.into();
        out
    }
}
