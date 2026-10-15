//! Operating-system integrations implementing `openflow_core::desktop::Desktop`,
//! plus global hotkey listeners and microphone capture.

pub mod capture;
pub mod keymap;
pub mod paths;
pub mod router;
pub mod utf16;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{SystemDesktop, spawn_hotkey_listener};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{SystemDesktop, accessibility_trusted, input_monitoring_granted, spawn_hotkey_listener};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{SystemDesktop, spawn_hotkey_listener};

/// Permission state shown in onboarding. Only macOS gates these behind user
/// consent; elsewhere they are always granted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permissions {
    pub accessibility: bool,
    pub input_monitoring: bool,
}

pub fn permissions(prompt: bool) -> Permissions {
    #[cfg(target_os = "macos")]
    {
        Permissions { accessibility: accessibility_trusted(prompt), input_monitoring: input_monitoring_granted(prompt) }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = prompt;
        Permissions { accessibility: true, input_monitoring: true }
    }
}
