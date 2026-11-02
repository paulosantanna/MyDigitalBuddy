// SPDX-License-Identifier: GPL-3.0-or-later
use super::{NativeWindow, Snapshot};
use crate::core::physics::Rect;
use std::{
    ffi::{CString, c_int, c_long, c_uchar, c_ulong},
    ptr,
};
use x11_dl::{xlib, xss};

unsafe extern "C" fn ignore_raced_window(
    _: *mut xlib::Display,
    _: *mut xlib::XErrorEvent,
) -> c_int {
    0
}

struct Display<'a> {
    x: &'a xlib::Xlib,
    d: *mut xlib::Display,
}
impl Drop for Display<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.x.XCloseDisplay)(self.d);
        }
    }
}
impl Display<'_> {
    fn atom(&self, name: &str) -> c_ulong {
        let Ok(c) = CString::new(name) else {
            return 0;
        };
        unsafe { (self.x.XInternAtom)(self.d, c.as_ptr(), 0) }
    }
    fn property(&self, w: c_ulong, name: &str) -> Vec<c_ulong> {
        unsafe {
            let mut actual = 0;
            let mut format = 0;
            let mut count = 0;
            let mut remaining = 0;
            let mut raw: *mut c_uchar = ptr::null_mut();
            let result = (self.x.XGetWindowProperty)(
                self.d,
                w,
                self.atom(name),
                0,
                4096,
                0,
                xlib::AnyPropertyType as c_ulong,
                &mut actual,
                &mut format,
                &mut count,
                &mut remaining,
                &mut raw,
            );
            let out = if result == 0 && !raw.is_null() && format == 32 {
                std::slice::from_raw_parts(raw.cast::<c_ulong>(), count as usize).to_vec()
            } else {
                Vec::new()
            };
            if !raw.is_null() {
                (self.x.XFree)(raw.cast());
            }
            out
        }
    }
    fn bounds(&self, w: c_ulong) -> Option<Rect> {
        unsafe {
            let mut attr: xlib::XWindowAttributes = std::mem::zeroed();
            if (self.x.XGetWindowAttributes)(self.d, w, &mut attr) == 0
                || attr.map_state != xlib::IsViewable
            {
                return None;
            }
            let mut x = 0;
            let mut y = 0;
            let mut child = 0;
            let root = (self.x.XDefaultRootWindow)(self.d);
            if (self.x.XTranslateCoordinates)(self.d, w, root, 0, 0, &mut x, &mut y, &mut child)
                == 0
            {
                return None;
            }
            let f = self.property(w, "_NET_FRAME_EXTENTS");
            let (l, r, t, b) = if f.len() >= 4 {
                (f[0] as f64, f[1] as f64, f[2] as f64, f[3] as f64)
            } else {
                (0.0, 0.0, 0.0, 0.0)
            };
            Some(Rect {
                left: x as f64 - l,
                top: y as f64 - t,
                right: x as f64 + attr.width as f64 + r,
                bottom: y as f64 + attr.height as f64 + b,
            })
        }
    }
}

pub fn sample() -> Snapshot {
    let Ok(x) = xlib::Xlib::open() else {
        return Snapshot {
            status: "X11 indisponível; pet e interações locais continuam funcionando".into(),
            ..Default::default()
        };
    };
    // SAFETY: this display belongs to the sampler thread and is closed by RAII.
    // XGetWindowProperty buffers are checked for type/length before use, then freed.
    unsafe {
        (x.XSetErrorHandler)(Some(ignore_raced_window));
        let d = (x.XOpenDisplay)(ptr::null());
        if d.is_null() {
            return Snapshot {
                status: "Sem sessão X11. Wayland exige XWayland para posicionar o pet.".into(),
                ..Default::default()
            };
        }
        let display = Display { x: &x, d };
        let root = (x.XDefaultRootWindow)(d);
        let mut out = Snapshot::default();
        let mut rr = 0;
        let mut cr = 0;
        let mut rx = 0;
        let mut ry = 0;
        let mut wx = 0;
        let mut wy = 0;
        let mut mask = 0;
        if (x.XQueryPointer)(
            d, root, &mut rr, &mut cr, &mut rx, &mut ry, &mut wx, &mut wy, &mut mask,
        ) != 0
        {
            out.cursor = (rx as f64, ry as f64);
        }
        let mut keys = [0_i8; 32];
        (x.XQueryKeymap)(d, keys.as_mut_ptr());
        let down = |sym: c_ulong| {
            let code = (x.XKeysymToKeycode)(d, sym) as usize;
            code > 0 && (keys[code / 8] as u8 & (1_u8 << (code % 8))) != 0
        };
        out.panic = (down(0xffe3) || down(0xffe4))
            && (down(0xffe9) || down(0xffea))
            && (down(0xffe1) || down(0xffe2))
            && down(0x62);
        if let Ok(ss) = xss::Xss::open() {
            let info = (ss.XScreenSaverAllocInfo)();
            if !info.is_null() {
                if (ss.XScreenSaverQueryInfo)(d, root, info) != 0 {
                    out.idle_seconds = Some((*info).idle as f64 / 1000.0);
                }
                (x.XFree)(info.cast());
            }
        }
        let active = display
            .property(root, "_NET_ACTIVE_WINDOW")
            .first()
            .copied()
            .unwrap_or(0);
        if active != 0 {
            if let Some(pid) = display.property(active, "_NET_WM_PID").first() {
                out.app = std::fs::read_to_string(format!("/proc/{pid}/comm"))
                    .unwrap_or_default()
                    .trim()
                    .to_lowercase();
            }
            out.fullscreen = display
                .property(active, "_NET_WM_STATE")
                .contains(&display.atom("_NET_WM_STATE_FULLSCREEN"));
        }
        let desktop = display
            .property(root, "_NET_CURRENT_DESKTOP")
            .first()
            .copied()
            .unwrap_or(0) as usize;
        let work = display.property(root, "_NET_WORKAREA");
        if let Some(w) = work.get(desktop * 4..desktop * 4 + 4) {
            out.work_area = Some(Rect {
                left: w[0] as c_long as f64,
                top: w[1] as c_long as f64,
                right: (w[0] + w[2]) as c_long as f64,
                bottom: (w[1] + w[3]) as c_long as f64,
            });
        } else {
            let screen = (x.XDefaultScreen)(d);
            out.work_area = Some(Rect {
                left: 0.0,
                top: 0.0,
                right: (x.XDisplayWidth)(d, screen) as f64,
                bottom: (x.XDisplayHeight)(d, screen) as f64,
            });
        }
        for id in display
            .property(root, "_NET_CLIENT_LIST_STACKING")
            .into_iter()
            .rev()
        {
            let pid = display
                .property(id, "_NET_WM_PID")
                .first()
                .copied()
                .unwrap_or(0);
            if pid == std::process::id() as c_ulong {
                continue;
            }
            if let Some(rect) = display.bounds(id)
                && rect.width() >= 160.0
                && rect.height() >= 80.0
            {
                let states = display.property(id, "_NET_WM_STATE");
                let maximized = states.contains(&display.atom("_NET_WM_STATE_MAXIMIZED_VERT"));
                out.windows.push(NativeWindow {
                    id,
                    rect,
                    maximized,
                });
            }
        }
        out.status = "X11: sensores locais ativos".into();
        out
    }
}
