//! Linux (X11): XInput2 raw key events for hotkeys (observe only, nothing can
//! be swallowed), XTEST for paste, EWMH for the active window, and the PRIMARY
//! selection as the "selected text". Wayland sessions work only for apps
//! running under XWayland.

use std::sync::Mutex;
use std::sync::mpsc::Sender;

use anyhow::{Context, anyhow};
use arboard::{GetExtLinux, LinuxClipboardKind};
use openflow_core::desktop::{AppInfo, Desktop, FieldSnapshot, Keystroke};
use openflow_core::hotkey::{HotkeyConfig, KeyEvent};
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xinput::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, Window};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

use crate::keymap::x11_keysym;
use crate::router::{KeyRouter, RecordingFlag};

const TERMINALS: &[&str] = &[
    "gnome-terminal-server",
    "konsole",
    "alacritty",
    "kitty",
    "xterm",
    "urxvt",
    "wezterm",
    "foot",
    "tilix",
    "terminator",
    "xfce4-terminal",
    "org.wezfurlong.wezterm",
    "ghostty",
];

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

fn atom(conn: &RustConnection, name: &str) -> anyhow::Result<u32> {
    Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
}

fn active_window(conn: &RustConnection, root: Window) -> anyhow::Result<Window> {
    let prop = conn
        .get_property(false, root, atom(conn, "_NET_ACTIVE_WINDOW")?, AtomEnum::WINDOW, 0, 1)?
        .reply()?;
    prop.value32().and_then(|mut v| v.next()).filter(|w| *w != 0).context("no active window")
}

fn string_property(conn: &RustConnection, window: Window, name: &str, ty: u32) -> Option<Vec<u8>> {
    let prop = conn.get_property(false, window, atom(conn, name).ok()?, ty, 0, 1024).ok()?.reply().ok()?;
    (!prop.value.is_empty()).then_some(prop.value)
}

fn read_active_app() -> anyhow::Result<AppInfo> {
    let (conn, screen) = x11rb::connect(None)?;
    let root = conn.setup().roots[screen].root;
    let window = active_window(&conn, root)?;
    let class = string_property(&conn, window, "WM_CLASS", AtomEnum::STRING.into()).and_then(|raw| {
        raw.split(|b| *b == 0).filter(|s| !s.is_empty()).nth(1).map(|s| String::from_utf8_lossy(s).to_lowercase())
    });
    let utf8 = atom(&conn, "UTF8_STRING")?;
    let name = string_property(&conn, window, "_NET_WM_NAME", utf8).map(|v| String::from_utf8_lossy(&v).into_owned());
    Ok(AppInfo { id: class, name, url: None })
}

fn keycode_for(conn: &RustConnection, keysym: u32) -> anyhow::Result<u8> {
    let setup = conn.setup();
    let (min, max) = (setup.min_keycode, setup.max_keycode);
    let map = conn.get_keyboard_mapping(min, max - min + 1)?.reply()?;
    let per = map.keysyms_per_keycode as usize;
    map.keysyms
        .chunks(per)
        .position(|syms| syms.contains(&keysym))
        .map(|i| min + i as u8)
        .ok_or_else(|| anyhow!("no keycode for keysym {keysym:#x}"))
}

fn press_chord(keysyms: &[u32]) -> anyhow::Result<()> {
    const KEY_PRESS: u8 = 2;
    const KEY_RELEASE: u8 = 3;
    let (conn, screen) = x11rb::connect(None)?;
    let root = conn.setup().roots[screen].root;
    let codes = keysyms.iter().map(|s| keycode_for(&conn, *s)).collect::<anyhow::Result<Vec<_>>>()?;
    for code in &codes {
        conn.xtest_fake_input(KEY_PRESS, *code, 0, root, 0, 0, 0)?;
    }
    for code in codes.iter().rev() {
        conn.xtest_fake_input(KEY_RELEASE, *code, 0, root, 0, 0, 0)?;
    }
    conn.get_input_focus()?.reply()?;
    Ok(())
}

const XK_CONTROL_L: u32 = 0xffe3;
const XK_SHIFT_L: u32 = 0xffe1;
const XK_RETURN: u32 = 0xff0d;
const XK_V: u32 = 0x76;
const XK_C: u32 = 0x63;

impl Desktop for SystemDesktop {
    fn frontmost_app(&self) -> AppInfo {
        read_active_app().unwrap_or_default()
    }

    fn ax_selected_text(&self) -> Option<String> {
        self.with_clipboard(|c| c.get().clipboard(LinuxClipboardKind::Primary).text().ok())
            .flatten()
            .filter(|s| !s.is_empty())
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
        let in_terminal = || self.frontmost_app().id.is_some_and(|id| TERMINALS.contains(&id.as_str()));
        match key {
            Keystroke::Paste if in_terminal() => press_chord(&[XK_CONTROL_L, XK_SHIFT_L, XK_V]),
            Keystroke::Paste => press_chord(&[XK_CONTROL_L, XK_V]),
            Keystroke::Copy if in_terminal() => press_chord(&[XK_CONTROL_L, XK_SHIFT_L, XK_C]),
            Keystroke::Copy => press_chord(&[XK_CONTROL_L, XK_C]),
            Keystroke::Enter => press_chord(&[XK_RETURN]),
        }
    }

    fn notify(&self, title: &str, body: &str) {
        let _ = notify_rust::Notification::new().appname("OpenFlow").summary(title).body(body).show();
    }
}

/// Watches raw key events on the root window through XInput2.
pub fn spawn_hotkey_listener(
    cfg: &HotkeyConfig,
    tx: Sender<KeyEvent>,
    recording: RecordingFlag,
) -> anyhow::Result<()> {
    let (conn, screen) = x11rb::connect(None).context("cannot connect to the X server (is DISPLAY set?)")?;
    let root = conn.setup().roots[screen].root;
    conn.xinput_xi_query_version(2, 0)?.reply().context("XInput2 is not available")?;
    let mask = xinput::XIEventMask::RAW_KEY_PRESS | xinput::XIEventMask::RAW_KEY_RELEASE;
    conn.xinput_xi_select_events(
        root,
        &[xinput::EventMask { deviceid: xinput::Device::ALL_MASTER.into(), mask: vec![mask] }],
    )?;
    conn.flush()?;
    let setup = conn.setup();
    let min = setup.min_keycode;
    let map = conn.get_keyboard_mapping(min, setup.max_keycode - min + 1)?.reply()?;
    let per = map.keysyms_per_keycode as usize;
    let keysym_of = move |code: u32| map.keysyms.get((code as usize).checked_sub(min as usize)? * per).copied();
    let mut router = KeyRouter::new(cfg, tx, recording);
    std::thread::Builder::new().name("openflow-hotkeys".into()).spawn(move || {
        while let Ok(event) = conn.wait_for_event() {
            let (code, down) = match event {
                Event::XinputRawKeyPress(e) => (e.detail, true),
                Event::XinputRawKeyRelease(e) => (e.detail, false),
                _ => continue,
            };
            if let Some(key) = keysym_of(code).and_then(x11_keysym) {
                router.on_key(key, down);
            }
        }
    })?;
    Ok(())
}
