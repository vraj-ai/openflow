//! Detects spoken enumerations ("first ... second ...") and numbers them.

use super::tidy;

const ORDINALS: [&str; 10] = [
    "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth",
];
const CARDINALS: [&str; 10] =
    ["one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten"];

#[derive(Clone)]
struct Cand {
    /// Index of the marker's first word.
    word: usize,
    /// Byte offset just past the marker.
    content: usize,
    num: usize,
}

pub fn format_lists(text: &str) -> String {
    let ws = words(text);
    let cands = candidates(text, &ws);
    match best_run(text, &ws, &cands) {
        Some(run) => render(text, &ws, &run),
        None => text.to_string(),
    }
}

fn candidates(text: &str, ws: &[(usize, usize)]) -> Vec<Cand> {
    let mut out = Vec::new();
    for (i, &(start, end)) in ws.iter().enumerate() {
        let w = text[start..end].to_lowercase();
        if w == "number" {
            if let Some(&(cs, ce)) = ws.get(i + 1) {
                let n = text[cs..ce].to_lowercase();
                if let Some(p) = CARDINALS.iter().position(|&c| c == n) {
                    out.push(Cand { word: i, content: ce, num: p + 1 });
                }
            }
            continue;
        }
        if let Some(p) = ORDINALS.iter().position(|&o| o == w) {
            out.push(Cand { word: i, content: end, num: p + 1 });
        }
    }
    out
}

/// Longest run of markers counting up from one; needs at least two.
fn best_run(text: &str, ws: &[(usize, usize)], cands: &[Cand]) -> Option<Vec<Cand>> {
    let mut best: Option<Vec<Cand>> = None;
    let mut i = 0;
    while i < cands.len() {
        if cands[i].num != 1 {
            i += 1;
            continue;
        }
        let mut run = vec![cands[i].clone()];
        let mut j = i + 1;
        while j < cands.len() && cands[j].num == run.len() + 1 && boundary_before(text, ws, &cands[j])
        {
            run.push(cands[j].clone());
            j += 1;
        }
        if run.len() >= 2 && best.as_ref().is_none_or(|b| b.len() < run.len()) {
            best = Some(run);
        }
        i = j;
    }
    best
}

/// A marker inside a sentence only counts when a comma or "and" precedes it,
/// so "a second opinion" stays prose.
fn boundary_before(text: &str, ws: &[(usize, usize)], c: &Cand) -> bool {
    let head = text[..ws[c.word].0].trim_end();
    match head.chars().next_back() {
        None => true,
        Some(',' | '.' | ';' | ':') => true,
        Some(_) => {
            let inner = words(head);
            inner.last().is_some_and(|&(a, b)| head[a..b].eq_ignore_ascii_case("and"))
        }
    }
}

fn render(text: &str, ws: &[(usize, usize)], run: &[Cand]) -> String {
    let mut out = String::new();
    let head = text[..ws[run[0].word].0].trim_end().trim_end_matches([',', '.', ';', ':']);
    if !head.trim().is_empty() {
        out.push_str(head.trim());
        out.push_str(":\n");
    }
    for (n, c) in run.iter().enumerate() {
        let (end, tail) = match run.get(n + 1) {
            Some(next) => (ws[next.word].0, None),
            None => {
                let e = sentence_end(text, c.content);
                (e, Some(if e < text.len() { e + 1 } else { text.len() }))
            }
        };
        out.push_str(&format!("{}. {}\n", n + 1, clean_item(&text[c.content..end])));
        if let Some(t) = tail {
            out.pop();
            let rest = text[t..].trim();
            if !rest.is_empty() {
                out.push_str("\n\n");
                out.push_str(rest);
            }
        }
    }
    tidy::tidy(&out)
}

fn sentence_end(s: &str, from: usize) -> usize {
    match s[from..].find(['.', '!', '?']) {
        Some(rel) => from + rel,
        None => s.len(),
    }
}

fn clean_item(s: &str) -> String {
    let mut t = s.trim_matches(|c: char| c.is_whitespace() || c == ',' || c == ';');
    loop {
        let trimmed = t.trim_end();
        if trimmed.len() != t.len() {
            t = trimmed;
            continue;
        }
        if matches!(t.chars().next_back(), Some(',' | ';' | '.')) {
            t = &t[..t.len() - 1];
            continue;
        }
        let inner = words(t);
        if let Some(&(a, b)) = inner.last()
            && t[a..b].eq_ignore_ascii_case("and")
        {
            t = t[..a].trim_end();
            continue;
        }
        return t.to_string();
    }
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
    fn ordinals_become_a_numbered_list() {
        assert_eq!(
            format_lists("My goals are first ship the app, second write the docs, and third rest."),
            "My goals are:\n1. Ship the app\n2. Write the docs\n3. Rest"
        );
    }

    #[test]
    fn number_one_two_becomes_a_numbered_list() {
        assert_eq!(
            format_lists("Number one, eggs. Number two, milk."),
            "1. Eggs\n2. Milk"
        );
    }

    #[test]
    fn a_later_sentence_is_kept_as_a_paragraph() {
        assert_eq!(
            format_lists("My goals are first a, second b, third c. Thanks all."),
            "My goals are:\n1. A\n2. B\n3. C\n\nThanks all."
        );
    }

    #[test]
    fn markers_may_start_a_new_sentence() {
        assert_eq!(format_lists("First, wash the car. Second, dry it."), "1. Wash the car\n2. Dry it");
    }

    #[test]
    fn a_single_marker_is_not_a_list() {
        assert_eq!(format_lists("First of all, thanks."), "First of all, thanks.");
        assert_eq!(format_lists("At first I hesitated."), "At first I hesitated.");
        assert_eq!(format_lists("The second time I called."), "The second time I called.");
    }

    #[test]
    fn a_second_in_prose_is_not_a_marker() {
        assert_eq!(
            format_lists("First I want a second opinion, then a third."),
            "First I want a second opinion, then a third."
        );
    }

    #[test]
    fn intro_keeps_a_trailing_comma_moved_to_a_colon() {
        assert_eq!(format_lists("Shopping list, first milk, second eggs."), "Shopping list:\n1. Milk\n2. Eggs");
        assert_eq!(format_lists("First, milk, second, eggs."), "1. Milk\n2. Eggs");
    }

    #[test]
    fn empty_and_no_change() {
        assert_eq!(format_lists(""), "");
        assert_eq!(format_lists("Just one plain sentence."), "Just one plain sentence.");
    }

    #[test]
    fn unicode_survives() {
        assert_eq!(
            format_lists("Ziele first äpfel kaufen, second bäume pflanzen."),
            "Ziele:\n1. Äpfel kaufen\n2. Bäume pflanzen"
        );
    }
}
