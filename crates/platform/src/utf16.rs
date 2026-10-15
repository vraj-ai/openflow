/// Accessibility APIs count UTF-16 code units; Rust strings index bytes.
pub fn utf16_range_to_bytes(text: &str, start: usize, len: usize) -> Option<std::ops::Range<usize>> {
    let mut units = 0usize;
    let mut from = None;
    for (byte, ch) in text.char_indices().chain(std::iter::once((text.len(), '\0'))) {
        if units == start {
            from = Some(byte);
        }
        if units == start + len {
            return Some(from?..byte);
        }
        units += ch.len_utf16();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_ranges_map_to_byte_ranges() {
        let text = "héllo 👋 world";
        assert_eq!(&text[utf16_range_to_bytes(text, 6, 2).unwrap()], "👋");
        assert_eq!(&text[utf16_range_to_bytes(text, 9, 5).unwrap()], "world");
        assert_eq!(utf16_range_to_bytes(text, 0, 0), Some(0..0));
        assert_eq!(utf16_range_to_bytes(text, 7, 1), None);
        assert_eq!(utf16_range_to_bytes(text, 40, 1), None);
    }
}
