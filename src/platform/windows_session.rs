// SPDX-License-Identifier: GPL-3.0-or-later
//! Notification area and the per-user "start with Windows" registration.
//! The message window lives on the UI thread, so winit's loop delivers its clicks.

use super::{TrayCommand, activation_object_name, startup_command};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_CLASS_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, GetLastError, HANDLE, HWND,
        LPARAM, LRESULT, POINT, WAIT_OBJECT_0, WPARAM,
    },
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
        DeleteObject,
    },
    System::{
        LibraryLoader::GetModuleHandleW,
        Registry::{
            HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, RegCloseKey,
            RegDeleteValueW, RegOpenKeyExW, RegSetValueExW,
        },
        Threading::{CreateEventW, EVENT_MODIFY_STATE, OpenEventW, SetEvent, WaitForSingleObject},
    },
    UI::{
        Shell::{
            NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
        },
        WindowsAndMessaging::{
            ASFW_ANY, AllowSetForegroundWindow, AppendMenuW, CreateIconIndirect, CreatePopupMenu,
            CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu, DestroyWindow, GetCursorPos,
            HWND_MESSAGE, ICONINFO, IDI_APPLICATION, LoadIconW, MB_ICONINFORMATION, MB_OK,
            MF_SEPARATOR, MF_STRING, MessageBoxW, PostMessageW, RegisterClassExW,
            SetForegroundWindow, TPM_BOTTOMALIGN, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTALIGN,
            TrackPopupMenu, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSEXW,
        },
    },
};

const ICON_ID: u32 = 1;
const WM_TRAY: u32 = 0x8007;
const CMD_OPEN: usize = 1;
const CMD_PAUSE: usize = 2;
const CMD_HIDE: usize = 3;
const CMD_PANIC: usize = 4;
const CMD_QUIT: usize = 5;
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_VALUE: &str = "MyDigitalBuddy";

static EVENTS: Mutex<Vec<TrayCommand>> = Mutex::new(Vec::new());
static PAUSED: AtomicBool = AtomicBool::new(false);
static HIDDEN: AtomicBool = AtomicBool::new(false);
static MENU_OPEN: AtomicBool = AtomicBool::new(false);

pub struct Tray {
    hwnd: HWND,
    icon: windows_sys::Win32::UI::WindowsAndMessaging::HICON,
    destroy_icon: bool,
}

impl Tray {
    pub fn install() -> Result<Self, String> {
        // SAFETY: the class and window are created on this thread; buffers outlive the calls.
        unsafe { install() }
    }

    pub fn poll(&mut self) -> Vec<TrayCommand> {
        let mut guard = EVENTS.lock().unwrap_or_else(|p| p.into_inner());
        std::mem::take(&mut *guard)
    }

    pub fn set_flags(&self, paused: bool, hidden: bool) {
        PAUSED.store(paused, Ordering::Relaxed);
        HIDDEN.store(hidden, Ordering::Relaxed);
    }
}

/// Auto-reset event the running instance waits on. A later launch sets it and exits.
pub struct Activation {
    event: HANDLE,
}

impl Activation {
    pub fn listen(root: &Path) -> Option<Self> {
        let name = wide(&activation_object_name(root));
        // SAFETY: `name` is NUL-terminated and outlives the call. The handle is owned here.
        let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
        if event.is_null() {
            None
        } else {
            Some(Self { event })
        }
    }

    pub fn poll(&self) -> bool {
        // SAFETY: `event` is the handle from `listen` and `Drop` has not closed it.
        unsafe { WaitForSingleObject(self.event, 0) == WAIT_OBJECT_0 }
    }
}

impl Drop for Activation {
    fn drop(&mut self) {
        // SAFETY: `event` is still open and owned by this value.
        unsafe {
            CloseHandle(self.event);
        }
    }
}

pub(super) fn request_running_instance(root: &Path) -> bool {
    let name = wide(&activation_object_name(root));
    for attempt in 0..10 {
        // SAFETY: `name` is NUL-terminated and outlives `OpenEventW`. The handle is closed here.
        let opened = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
        if !opened.is_null() {
            let ok = unsafe {
                // The process the user just started may hand foreground to the one already running.
                AllowSetForegroundWindow(ASFW_ANY);
                let ok = SetEvent(opened) != 0;
                CloseHandle(opened);
                ok
            };
            if ok {
                return true;
            }
        }
        if attempt + 1 < 10 {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    false
}

pub(super) fn notify_running_instance_unresponsive() {
    let title = wide("MyDigitalBuddy");
    let text = wide(
        "O MyDigitalBuddy já está aberto, mas a janela não voltou. Clique com o botão direito no ícone da bandeja, escolha Sair e abra de novo.",
    );
    // SAFETY: both strings are NUL-terminated and outlive MessageBoxW.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        // SAFETY: hwnd and icon were created by install and are still ours.
        unsafe {
            let data = icon_data(self.hwnd, self.icon);
            Shell_NotifyIconW(NIM_DELETE, &raw const data);
            DestroyWindow(self.hwnd);
            if self.destroy_icon {
                DestroyIcon(self.icon);
            }
        }
    }
}

pub(super) fn set_launch_at_startup(enabled: bool) -> Result<(), String> {
    let command = if enabled {
        let exe = std::env::current_exe().map_err(|e| format!("executável: {e}"))?;
        Some(startup_command(&exe.display().to_string())?)
    } else {
        None
    };
    // SAFETY: the key is HKCU Run, opened and closed around this call; strings are NUL-terminated.
    unsafe { write_run_value(command.as_deref()) }
}

unsafe fn install() -> Result<Tray, String> {
    unsafe {
        let class = class_name();
        let instance = GetModuleHandleW(std::ptr::null());
        let wnd = WNDCLASSEXW {
            cbSize: u32::try_from(std::mem::size_of::<WNDCLASSEXW>()).unwrap_or(80),
            lpfnWndProc: Some(tray_wndproc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..zeroed_class()
        };
        if RegisterClassExW(&raw const wnd) == 0 {
            let err = GetLastError();
            if err != ERROR_CLASS_ALREADY_EXISTS {
                return Err(format!("bandeja: classe da janela ({err})"));
            }
        }
        let title = wide("MyDigitalBuddy");
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return Err(format!("bandeja: janela ({})", GetLastError()));
        }
        let (icon, destroy_icon) = match buddy_icon() {
            Some(icon) => (icon, true),
            None => {
                let shared = LoadIconW(std::ptr::null_mut(), IDI_APPLICATION);
                if shared.is_null() {
                    DestroyWindow(hwnd);
                    return Err("bandeja: ícone".into());
                }
                (shared, false)
            }
        };
        let data = icon_data(hwnd, icon);
        if Shell_NotifyIconW(NIM_ADD, &raw const data) == 0 {
            if destroy_icon {
                DestroyIcon(icon);
            }
            DestroyWindow(hwnd);
            return Err(format!("bandeja: Shell_NotifyIcon ({})", GetLastError()));
        }
        Ok(Tray {
            hwnd,
            icon,
            destroy_icon,
        })
    }
}

fn zeroed_class() -> WNDCLASSEXW {
    // SAFETY: WNDCLASSEXW is a plain C struct; every field is overwritten or left at zero.
    unsafe { std::mem::zeroed() }
}

fn class_name() -> &'static [u16] {
    static NAME: OnceLock<Vec<u16>> = OnceLock::new();
    NAME.get_or_init(|| wide("MyDigitalBuddyTray"))
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn icon_data(
    hwnd: HWND,
    icon: windows_sys::Win32::UI::WindowsAndMessaging::HICON,
) -> NOTIFYICONDATAW {
    // SAFETY: NOTIFYICONDATAW is a plain C struct. Only the fields Shell_NotifyIcon reads are set.
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = u32::try_from(std::mem::size_of::<NOTIFYICONDATAW>()).unwrap_or(0);
    data.hWnd = hwnd;
    data.uID = ICON_ID;
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.uCallbackMessage = WM_TRAY;
    data.hIcon = icon;
    let tip = wide("MyDigitalBuddy");
    let n = tip.len().min(data.szTip.len());
    data.szTip[..n].copy_from_slice(&tip[..n]);
    data
}

fn buddy_icon() -> Option<windows_sys::Win32::UI::WindowsAndMessaging::HICON> {
    const N: i32 = 32;
    let pixels = crate::sophia_icon::bgra(N as usize);
    // SAFETY: the DIB and mask live until CreateIconIndirect copies them; both are then deleted.
    unsafe {
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: u32::try_from(std::mem::size_of::<BITMAPINFOHEADER>()).unwrap_or(40),
            biWidth: N,
            biHeight: -N,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let color = CreateDIBSection(
            std::ptr::null_mut(),
            &raw const info,
            DIB_RGB_COLORS,
            &raw mut bits,
            std::ptr::null_mut(),
            0,
        );
        if color.is_null() || bits.is_null() {
            return None;
        }
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast(), pixels.len());
        let mask_bits = [0_u8; 128];
        let mask = CreateBitmap(N, N, 1, 1, mask_bits.as_ptr().cast());
        if mask.is_null() {
            DeleteObject(color);
            return None;
        }
        let icon_info = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&raw const icon_info);
        DeleteObject(color);
        DeleteObject(mask);
        if icon.is_null() { None } else { Some(icon) }
    }
}

unsafe extern "system" fn tray_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if msg == WM_TRAY {
            on_tray(hwnd, lparam);
            0
        } else {
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
    }
}

fn on_tray(hwnd: HWND, lparam: LPARAM) {
    let mouse = u32::try_from(lparam).unwrap_or(0);
    if mouse == WM_LBUTTONDBLCLK || mouse == WM_LBUTTONUP {
        push(TrayCommand::Open);
    } else if mouse == WM_RBUTTONUP {
        show_menu(hwnd);
    }
}

fn push(command: TrayCommand) {
    let mut guard = EVENTS.lock().unwrap_or_else(|p| p.into_inner());
    guard.push(command);
}

fn show_menu(hwnd: HWND) {
    if MENU_OPEN.swap(true, Ordering::Relaxed) {
        return;
    }
    let paused = PAUSED.load(Ordering::Relaxed);
    let hidden = HIDDEN.load(Ordering::Relaxed);
    let open = wide("Abrir central");
    let pause = wide(if paused { "Continuar" } else { "Pausar" });
    let hide = wide(if hidden { "Mostrar" } else { "Esconder" });
    let panic = wide("Pânico");
    let quit = wide("Sair");
    // SAFETY: menu strings outlive TrackPopupMenu; the menu is destroyed before return.
    let chosen = unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            MENU_OPEN.store(false, Ordering::Relaxed);
            return;
        }
        AppendMenuW(menu, MF_STRING, CMD_OPEN, open.as_ptr());
        AppendMenuW(menu, MF_STRING, CMD_PAUSE, pause.as_ptr());
        AppendMenuW(menu, MF_STRING, CMD_HIDE, hide.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, CMD_PANIC, panic.as_ptr());
        AppendMenuW(menu, MF_STRING, CMD_QUIT, quit.as_ptr());
        let mut point = POINT { x: 0, y: 0 };
        GetCursorPos(&raw mut point);
        SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenu(
            menu,
            TPM_RIGHTALIGN | TPM_BOTTOMALIGN | TPM_RETURNCMD | TPM_NONOTIFY,
            point.x,
            point.y,
            0,
            hwnd,
            std::ptr::null(),
        );
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        chosen
    };
    MENU_OPEN.store(false, Ordering::Relaxed);
    let Some(id) = usize::try_from(chosen).ok() else {
        return;
    };
    let command = match id {
        CMD_OPEN => TrayCommand::Open,
        CMD_PAUSE => TrayCommand::TogglePause,
        CMD_HIDE => TrayCommand::ToggleHidden,
        CMD_PANIC => TrayCommand::Panic,
        CMD_QUIT => TrayCommand::Quit,
        _ => return,
    };
    push(command);
}

unsafe fn write_run_value(command: Option<&str>) -> Result<(), String> {
    unsafe {
        let subkey = wide(RUN_KEY);
        let name = wide(RUN_VALUE);
        let mut key = std::ptr::null_mut();
        let opened = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            KEY_SET_VALUE | KEY_QUERY_VALUE,
            &raw mut key,
        );
        if opened != 0 {
            return Err(format!("registro de inicialização: {opened}"));
        }
        let status = if let Some(command) = command {
            let value = wide(command);
            let bytes = u32::try_from(value.len().saturating_mul(2)).unwrap_or(0);
            RegSetValueExW(key, name.as_ptr(), 0, REG_SZ, value.as_ptr().cast(), bytes)
        } else {
            let deleted = RegDeleteValueW(key, name.as_ptr());
            if deleted == ERROR_FILE_NOT_FOUND {
                0
            } else {
                deleted
            }
        };
        RegCloseKey(key);
        if status == 0 {
            Ok(())
        } else {
            Err(format!("registro de inicialização: {status}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Activation, request_running_instance};

    #[test]
    fn show_request_wakes_the_running_instance() {
        let root =
            std::env::temp_dir().join(format!("mydigitalbuddy-activation-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&root);
        let woke = if let Some(listener) = Activation::listen(&root) {
            let idle = listener.poll();
            let signaled = request_running_instance(&root);
            let seen = listener.poll();
            let consumed = listener.poll();
            !idle && signaled && seen && !consumed
        } else {
            false
        };
        let _ = std::fs::remove_dir_all(&root);
        assert!(woke);
    }
}
