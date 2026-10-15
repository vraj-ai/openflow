//! Per-OS key code tables. Kept free of OS APIs so they are tested everywhere.

use openflow_core::hotkey::Key;

/// macOS virtual key codes (HIToolbox `kVK_*`).
pub fn mac_keycode(code: u16) -> Option<Key> {
    Some(match code {
        63 => Key::Fn,
        59 => Key::Ctrl,
        62 => Key::RightCtrl,
        58 => Key::Alt,
        61 => Key::RightAlt,
        56 | 60 => Key::Shift,
        55 | 54 => Key::Meta,
        49 => Key::Space,
        53 => Key::Escape,
        122 => Key::F(1),
        120 => Key::F(2),
        99 => Key::F(3),
        118 => Key::F(4),
        96 => Key::F(5),
        97 => Key::F(6),
        98 => Key::F(7),
        100 => Key::F(8),
        101 => Key::F(9),
        109 => Key::F(10),
        103 => Key::F(11),
        111 => Key::F(12),
        _ => return None,
    })
}

/// Windows virtual-key codes as reported by the low-level keyboard hook.
pub fn windows_vk(vk: u32) -> Option<Key> {
    Some(match vk {
        0xA2 | 0x11 => Key::Ctrl,
        0xA3 => Key::RightCtrl,
        0xA4 | 0x12 => Key::Alt,
        0xA5 => Key::RightAlt,
        0xA0 | 0xA1 | 0x10 => Key::Shift,
        0x5B | 0x5C => Key::Meta,
        0x20 => Key::Space,
        0x1B => Key::Escape,
        0x70..=0x7B => Key::F((vk - 0x70 + 1) as u8),
        _ => return None,
    })
}

/// X11 keysyms.
pub fn x11_keysym(sym: u32) -> Option<Key> {
    Some(match sym {
        0xffe3 => Key::Ctrl,
        0xffe4 => Key::RightCtrl,
        0xffe9 => Key::Alt,
        0xffea | 0xfe03 => Key::RightAlt,
        0xffe1 | 0xffe2 => Key::Shift,
        0xffeb | 0xffec | 0xffe7 | 0xffe8 => Key::Meta,
        0x20 => Key::Space,
        0xff1b => Key::Escape,
        0xffbe..=0xffc9 => Key::F((sym - 0xffbe + 1) as u8),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_cover_default_chords() {
        assert_eq!(mac_keycode(63), Some(Key::Fn));
        assert_eq!(mac_keycode(59), Some(Key::Ctrl));
        assert_eq!(mac_keycode(49), Some(Key::Space));
        assert_eq!(windows_vk(0xA2), Some(Key::Ctrl));
        assert_eq!(windows_vk(0x5B), Some(Key::Meta));
        assert_eq!(windows_vk(0x7B), Some(Key::F(12)));
        assert_eq!(x11_keysym(0xffeb), Some(Key::Meta));
        assert_eq!(x11_keysym(0xffbe), Some(Key::F(1)));
        assert_eq!(x11_keysym(0x61), None);
    }
}
