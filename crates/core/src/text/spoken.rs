//! Turns "new line" / "new paragraph" into real line breaks.

use super::tidy;

pub fn apply_spoken_commands(text: &str) -> String {
    let breaks = breaks(text);
    if breaks.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    for (a, b, rep) in breaks {
        let a = a.max(pos);
        if a >= b {
            continue;
        }
        out.push_str(&text[pos..a]);
        out.push_str(rep);
        pos = b;
    }
    out.push_str(&text[pos..]);
    tidy::tidy(&out)
}

fn breaks(text: &str) -> Vec<(usize, usize, &'static str)> {
    let ws = words(text);
    let mut out = Vec::new();
    let mut i = 0;
    while i < ws.len() {
        if text[ws[i].0..ws[i].1].eq_ignore_ascii_case("new") && i + 1 < ws.len() {
            let rep = match text[ws[i + 1].0..ws[i + 1].1].to_lowercase().as_str() {
                "line" => Some("\n"),
                "paragraph" => Some("\n\n"),
                _ => None,
            };
            if let Some(rep) = rep {
                let (a, b) = widen(text, ws[i].0, ws[i + 1].1);
                out.push((a, b, rep));
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// The phrase plus the spaces and comma/period glued to it; a comma in front of
/// the phrase (as in "Hi team, new paragraph") is kept.
fn widen(s: &str, start: usize, end: usize) -> (usize, usize) {
    let mut a = start;
    while a > 0 && matches!(s[..a].chars().next_back(), Some(' ') | Some('\t')) {
        a -= 1;
    }
    let mut b = end;
    while b < s.len() && matches!(s[b..].chars().next(), Some(' ') | Some('\t')) {
        b += 1;
    }
    if b < s.len() && matches!(s[b..].chars().next(), Some(',') | Some('.')) {
        b += 1;
    }
    while b < s.len() && matches!(s[b..].chars().next(), Some(' ') | Some('\t')) {
        b += 1;
    }
    (a, b)
}

fn words(s: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in s.char_indices() {
        if c.is_alphanumeric() || c == '\'' || c == '-' {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(st) = start.take() {
            out.push((st, i));
        }
    }
    if let Some(st) = start {
        out.push((st, s.len()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_paragraph_breaks_and_capitalizes() {
        assert_eq!(
            apply_spoken_commands("Hi team, new paragraph, the launch is Friday."),
            "Hi team,\n\nThe launch is Friday."
        );
    }

    #[test]
    fn new_line_breaks_and_capitalizes() {
        assert_eq!(
            apply_spoken_commands("first item, new line, second item."),
            "First item,\nSecond item."
        );
        assert_eq!(apply_spoken_commands("Hi, new line, how are you?"), "Hi,\nHow are you?");
    }

    #[test]
    fn phrase_case_is_ignored() {
        assert_eq!(apply_spoken_commands("Done. New Line. Next up."), "Done.\nNext up.");
    }

    #[test]
    fn press_enter_is_not_handled() {
        assert_eq!(apply_spoken_commands("Hi, press enter, there."), "Hi, press enter, there.");
    }

    #[test]
    fn no_break_words_leaves_text_alone() {
        assert_eq!(apply_spoken_commands("We need a fresh start."), "We need a fresh start.");
        assert_eq!(apply_spoken_commands("One two three."), "One two three.");
    }

    #[test]
    fn empty_and_unicode() {
        assert_eq!(apply_spoken_commands(""), "");
        assert_eq!(
            apply_spoken_commands("Grüße, new paragraph, alles gut."),
            "Grüße,\n\nAlles gut."
        );
    }
}
