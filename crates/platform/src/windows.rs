//! Windows: WH_KEYBOARD_LL hook for hotkeys, SendInput for paste, the
//! foreground window's executable name as the app id. There is no UI
//! Automation reader yet, so selection always uses the copy fallback.

use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};

use anyhow::anyhow;
use openflow_core::desktop::{AppInfo, Desktop, FieldSnapshot, Keystroke};
use openflow_core::hotkey::{HotkeyConfig, KeyEvent};
use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_CONTROL, VK_RETURN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowTextW, GetWindowThreadProcessId,
    KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, SetWindowsHookExW, TranslateMessage, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
    WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::keymap::windows_vk;
use crate::router::{KeyRouter, RecordingFlag};

#[derive(Default)]
pub struct SystemDesktop {
    clipboard: Mutex<Option<arboard::Clipboard>>,
}

impl SystemDesktop {
    pub fn new() -> Self {
        Self::default()
    }

    fn with_clipboard<R>(&self, f: impl FnOnce(&mut arboard::Clipboard) -> R) -> Option<R> {
        let mut guard = self.clipboard.lock().unwrap();
        if guard.is_none() {
            *guard = arboard::Clipboard::new().ok();
        }
        guard.as_mut().map(f)
    }
}

fn key_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { 0 }, time: 0, dwExtraInfo: 0 },
        },
    }
}

fn process_name(hwnd: HWND) -> Option<String> {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid == 0 {
        return None;
    }
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) };
    unsafe { CloseHandle(handle) };
    if ok == 0 {
        return None;
    }
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    path.rsplit('\\').next().map(|s| s.to_ascii_lowercase())
}

impl Desktop for SystemDesktop {
    fn frontmost_app(&self) -> AppInfo {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_null() {
            return AppInfo::default();
        }
        let mut title = [0u16; 512];
        let n = unsafe { GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32) };
        let name = (n > 0).then(|| String::from_utf16_lossy(&title[..n as usize]));
        AppInfo { id: process_name(hwnd), name, url: None }
    }

    fn ax_selected_text(&self) -> Option<String> {
        None
    }

    fn focused_field(&self) -> Option<FieldSnapshot> {
        None
    }

    fn clipboard_text(&self) -> Option<String> {
        self.with_clipboard(|c| c.get_text().ok()).flatten()
    }

    fn set_clipboard_text(&self, text: &str) -> anyhow::Result<()> {
        self.with_clipboard(|c| c.set_text(text.to_string()))
            .ok_or_else(|| anyhow!("clipboard unavailable"))??;
        Ok(())
    }

    fn send(&self, key: Keystroke) -> anyhow::Result<()> {
        let inputs: Vec<INPUT> = match key {
            Keystroke::Paste => vec![
                key_input(VK_CONTROL, false),
                key_input(b'V' as u16, false),
                key_input(b'V' as u16, true),
                key_input(VK_CONTROL, true),
            ],
            Keystroke::Copy => vec![
                key_input(VK_CONTROL, false),
                key_input(b'C' as u16, false),
                key_input(b'C' as u16, true),
                key_input(VK_CONTROL, true),
            ],
            Keystroke::Enter => vec![key_input(VK_RETURN, false), key_input(VK_RETURN, true)],
        };
        let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) };
        if sent as usize != inputs.len() {
            return Err(anyhow!("SendInput delivered {sent} of {} events", inputs.len()));
        }
        Ok(())
    }

    fn notify(&self, title: &str, body: &str) {
        let _ = notify_rust::Notification::new().appname("OpenFlow").summary(title).body(body).show();
    }
}

static ROUTER: OnceLock<Mutex<KeyRouter>> = OnceLock::new();

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        let injected = info.flags & LLKHF_INJECTED != 0;
        let down = matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        let up = matches!(wparam as u32, WM_KEYUP | WM_SYSKEYUP);
        if !injected && (down || up) {
            if let (Some(key), Some(router)) = (windows_vk(info.vkCode), ROUTER.get()) {
                if router.lock().unwrap().on_key(key, down) {
                    return 1;
                }
            }
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// Installs a low-level keyboard hook on a thread with its own message loop.
/// Only one listener can exist per process.
pub fn spawn_hotkey_listener(
    cfg: &HotkeyConfig,
    tx: Sender<KeyEvent>,
    recording: RecordingFlag,
) -> anyhow::Result<()> {
    ROUTER
        .set(Mutex::new(KeyRouter::new(cfg, tx, recording)))
        .map_err(|_| anyhow!("hotkey listener already running"))?;
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    std::thread::Builder::new().name("openflow-hotkeys".into()).spawn(move || unsafe {
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), GetModuleHandleW(std::ptr::null()), 0);
        if hook.is_null() {
            let _ = ready_tx.send(Err(anyhow!("SetWindowsHookExW failed")));
            return;
        }
        let _ = ready_tx.send(Ok(()));
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    })?;
    ready_rx.recv()?
}
