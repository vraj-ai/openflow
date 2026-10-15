//! macOS: CGEventTap hotkeys (the only way to see Fn held down), AX API for
//! selection and field text, NSWorkspace for the frontmost app, CGEvent paste.

use std::ffi::c_void;
use std::process::Command;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use anyhow::anyhow;
use core_foundation::base::{CFRelease, CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::runloop::CFRunLoop;
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, CallbackResult, EventField,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use objc2_app_kit::NSWorkspace;
use openflow_core::desktop::{AppInfo, Desktop, FieldSnapshot, Keystroke};
use openflow_core::hotkey::{HotkeyConfig, Key, KeyEvent};

use crate::keymap::mac_keycode;
use crate::router::{KeyRouter, RecordingFlag};
use crate::utf16::utf16_range_to_bytes;

type AXUIElementRef = *const c_void;
type AXValueRef = *const c_void;

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct CFRange {
    location: isize,
    length: isize,
}

fn cf_range_to_bytes(text: &str, range: CFRange) -> Option<std::ops::Range<usize>> {
    utf16_range_to_bytes(text, usize::try_from(range.location).ok()?, usize::try_from(range.length).ok()?)
}

const K_AX_VALUE_CFRANGE_TYPE: u32 = 4;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(element: AXUIElementRef, attribute: CFStringRef, value: *mut CFTypeRef) -> i32;
    fn AXValueGetValue(value: AXValueRef, the_type: u32, out: *mut c_void) -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
}

/// An owned CF object returned by a Copy/Create function.
struct Owned(CFTypeRef);

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

fn ax_attribute(element: CFTypeRef, name: &str) -> Option<Owned> {
    let name = CFString::new(name);
    let mut value: CFTypeRef = std::ptr::null();
    let err = unsafe { AXUIElementCopyAttributeValue(element, name.as_concrete_TypeRef(), &mut value) };
    (err == 0 && !value.is_null()).then(|| Owned(value))
}

fn focused_element() -> Option<Owned> {
    let system = Owned(unsafe { AXUIElementCreateSystemWide() });
    ax_attribute(system.0, "AXFocusedUIElement")
}

fn cf_string(value: &Owned) -> Option<String> {
    let ty = unsafe { CFType::wrap_under_get_rule(value.0) };
    ty.downcast::<CFString>().map(|s| s.to_string())
}

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

fn browser_url(bundle_id: &str) -> Option<String> {
    let script = match bundle_id {
        "com.apple.Safari" => r#"tell application "Safari" to get URL of front document"#,
        "com.google.Chrome" => r#"tell application "Google Chrome" to get URL of active tab of front window"#,
        "com.brave.Browser" => r#"tell application "Brave Browser" to get URL of active tab of front window"#,
        "com.microsoft.edgemac" => r#"tell application "Microsoft Edge" to get URL of active tab of front window"#,
        "company.thebrowser.Browser" => r#"tell application "Arc" to get URL of active tab of front window"#,
        _ => return None,
    };
    let out = Command::new("osascript").args(["-e", script]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

impl Desktop for SystemDesktop {
    fn frontmost_app(&self) -> AppInfo {
        let workspace = NSWorkspace::sharedWorkspace();
        let Some(app) = workspace.frontmostApplication() else { return AppInfo::default() };
        let id = app.bundleIdentifier().map(|s| s.to_string());
        let name = app.localizedName().map(|s| s.to_string());
        let url = id.as_deref().and_then(browser_url);
        AppInfo { id, name, url }
    }

    fn ax_selected_text(&self) -> Option<String> {
        let element = focused_element()?;
        let value = ax_attribute(element.0, "AXSelectedText")?;
        cf_string(&value).filter(|s| !s.is_empty())
    }

    fn focused_field(&self) -> Option<FieldSnapshot> {
        let element = focused_element()?;
        let value = cf_string(&ax_attribute(element.0, "AXValue")?)?;
        let selection = ax_attribute(element.0, "AXSelectedTextRange").and_then(|range| {
            let mut out = CFRange::default();
            let ok = unsafe {
                AXValueGetValue(range.0, K_AX_VALUE_CFRANGE_TYPE, &mut out as *mut CFRange as *mut c_void)
            };
            ok.then(|| cf_range_to_bytes(&value, out)).flatten()
        });
        Some(FieldSnapshot { value, selection })
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
        let (code, flags) = match key {
            Keystroke::Paste => (9, CGEventFlags::CGEventFlagCommand),
            Keystroke::Copy => (8, CGEventFlags::CGEventFlagCommand),
            Keystroke::Enter => (36, CGEventFlags::CGEventFlagNull),
        };
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|_| anyhow!("CGEventSource unavailable"))?;
        for down in [true, false] {
            let event = CGEvent::new_keyboard_event(source.clone(), code, down)
                .map_err(|_| anyhow!("CGEvent creation failed"))?;
            event.set_flags(flags);
            event.post(CGEventTapLocation::HID);
        }
        Ok(())
    }

    fn notify(&self, title: &str, body: &str) {
        let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(r#"display notification "{}" with title "{}""#, quote(body), quote(title));
        let _ = Command::new("osascript").args(["-e", &script]).spawn();
    }
}

/// Modifier keys arrive as FlagsChanged events; whether it is a press or a
/// release has to be read from the flag the key controls.
fn modifier_down(key: Key, flags: CGEventFlags) -> bool {
    let mask = match key {
        Key::Fn => CGEventFlags::CGEventFlagSecondaryFn,
        Key::Ctrl | Key::RightCtrl => CGEventFlags::CGEventFlagControl,
        Key::Alt | Key::RightAlt => CGEventFlags::CGEventFlagAlternate,
        Key::Shift => CGEventFlags::CGEventFlagShift,
        Key::Meta => CGEventFlags::CGEventFlagCommand,
        _ => return false,
    };
    flags.contains(mask)
}

/// Installs a CGEventTap on a dedicated run-loop thread. Requires the Input
/// Monitoring permission (and Accessibility to swallow events).
pub fn spawn_hotkey_listener(
    cfg: &HotkeyConfig,
    tx: Sender<KeyEvent>,
    recording: RecordingFlag,
) -> anyhow::Result<()> {
    let router = Arc::new(Mutex::new(KeyRouter::new(cfg, tx, recording)));
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    std::thread::Builder::new().name("openflow-hotkeys".into()).spawn(move || {
        let callback = move |_proxy, ty: CGEventType, event: &CGEvent| {
            let code = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16;
            let Some(key) = mac_keycode(code) else { return CallbackResult::Keep };
            let down = match ty {
                CGEventType::KeyDown => true,
                CGEventType::KeyUp => false,
                CGEventType::FlagsChanged => modifier_down(key, event.get_flags()),
                _ => return CallbackResult::Keep,
            };
            if router.lock().unwrap().on_key(key, down) { CallbackResult::Drop } else { CallbackResult::Keep }
        };
        let result = CGEventTap::with_enabled(
            CGEventTapLocation::HID,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Default,
            vec![CGEventType::KeyDown, CGEventType::KeyUp, CGEventType::FlagsChanged],
            callback,
            || {
                let _ = ready_tx.send(Ok(()));
                CFRunLoop::run_current();
            },
        );
        if result.is_err() {
            let _ = ready_tx.send(Err(anyhow!(
                "could not create the keyboard event tap; grant Input Monitoring and Accessibility in System Settings > Privacy & Security"
            )));
        }
    })?;
    ready_rx.recv()?
}

pub fn accessibility_trusted(prompt: bool) -> bool {
    let key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
    let options = CFDictionary::from_CFType_pairs(&[(key, CFBoolean::from(prompt))]);
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef() as *const c_void) }
}

pub fn input_monitoring_granted(prompt: bool) -> bool {
    unsafe { if prompt { CGRequestListenEventAccess() } else { CGPreflightListenEventAccess() } }
}
