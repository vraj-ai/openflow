//! Applies spoken self-corrections: "no wait", "sorry", "scratch that", ...

use super::tidy;

/// Markers that replace what came before them with the words that follow.
const MARKERS: [&[&str]; 7] = [
    &["no", "wait"],
    &["wait", "no"],
    &["no", "actually"],
    &["sorry"],
    &["i", "mean"],
    &["make", "that"],
    &["or", "rather"],
];

/// Markers that retract the preceding sentence (or clause).
const DROP_MARKERS: [&[&str]; 3] = [&["scratch", "that"], &["delete", "that"], &["never", "mind", "that"]];

pub fn apply_self_corrections(text: &str) -> String {
    let mut s = text.to_string();
    // Every pass removes at least one word, so this terminates; the bound is
    // just belt and braces.
    for _ in 0..64 {
        match apply_once(&s) {
            Some(next) => s = next,
            None => break,
        }
    }
    tidy::tidy(&s)
}

struct Word {
    start: usize,
    end: usize,
    lower: String,
    sentence: usize,
}

fn apply_once(s: &str) -> Option<String> {
    let ws = scan(s);
    for i in 0..ws.len() {
        for m in DROP_MARKERS {
            if let Some(j) = match_at(&ws, i, m)
                && let Some(out) = drop_region(s, &ws, i, j)
            {
                return Some(out);
            }
        }
        for m in MARKERS {
            let Some(j) = match_at(&ws, i, m) else { continue };
            let sentence = ws[i].sentence;
            let before: Vec<usize> = (0..i).filter(|&k| ws[k].sentence == sentence).collect();
            if before.is_empty() {
                continue;
            }
            let r_end = (j..ws.len()).take_while(|&k| ws[k].sentence == sentence).count() + j;
            if r_end == j {
                continue;
            }
            if m.len() == 2 && m[0] == "i" && m[1] == "mean" && r_end - j > 5 {
                continue;
            }
            let first = ws[j].lower.clone();
            let cut = before
                .iter()
                .rev()
                .find(|&&k| ws[k].lower == first)
                .copied()
                .unwrap_or(before[before.len() - (r_end - j).min(before.len())]);
            return Some(join(&s[..ws[cut].start], &s[ws[j].start..]));
        }
    }
    None
}

/// "scratch that" retracts the sentence it sits in, or the sentence before it
/// when it starts its own.
fn drop_region(s: &str, ws: &[Word], i: usize, j: usize) -> Option<String> {
    let sentence = ws[i].sentence;
    let before: Vec<usize> = (0..i).filter(|&k| ws[k].sentence == sentence).collect();
    let start = if before.is_empty() {
        let k = (0..i).rev().find(|&k| ws[k].sentence < sentence)?;
        let first = (0..=k).find(|&p| ws[p].sentence == ws[k].sentence).unwrap();
        ws[first].start
    } else {
        // The clause before the last delimiter ahead of the marker.
        let head = ws[before[0]].start;
        let mut delims = s[head..ws[i].start].match_indices([',', ';']).map(|(o, _)| head + o);
        delims.next_back();
        match delims.next_back() {
            Some(prev) => prev + 1,
            None => head,
        }
    };
    let mut end = ws[j - 1].end;
    let mut seen = false;
    for c in s[end..].chars() {
        if c == ' ' || c == '\t' {
            end += c.len_utf8();
        } else if !seen && matches!(c, ',' | '.' | '!' | '?') {
            seen = true;
            end += c.len_utf8();
        } else {
            break;
        }
    }
    Some(join(&s[..start], &s[end..]))
}

/// Glues two fragments with a space only when nothing else separates them.
fn join(prefix: &str, suffix: &str) -> String {
    let (Some(last), Some(first)) = (prefix.chars().next_back(), suffix.chars().next()) else {
        return format!("{prefix}{suffix}");
    };
    let open = last.is_whitespace() || matches!(last, '(' | '[' | '{' | '"' | '\'');
    let close = first.is_whitespace() || matches!(first, ',' | '.' | '!' | '?' | ';' | ':' | ')' | ']' | '}');
    if open || close { format!("{prefix}{suffix}") } else { format!("{prefix} {suffix}") }
}

fn match_at(ws: &[Word], i: usize, m: &[&str]) -> Option<usize> {
    let end = i + m.len();
    if end > ws.len() {
        return None;
    }
    let same = ws[i..end].iter().all(|w| w.sentence == ws[i].sentence);
    let fits = ws[i..end].iter().zip(m).all(|(w, m)| w.lower == *m);
    (same && fits).then_some(end)
}

fn scan(s: &str) -> Vec<Word> {
    let mut out: Vec<Word> = Vec::new();
    let mut sentence = 0;
    let mut start: Option<usize> = None;
    for (i, c) in s.char_indices() {
        if c.is_alphanumeric() || c == '\'' || c == '-' {
            if start.is_none() {
                start = Some(i);
            }
            continue;
        }
        if let Some(st) = start.take() {
            out.push(Word { start: st, end: i, lower: s[st..i].to_lowercase(), sentence });
        }
        if matches!(c, '.' | '!' | '?') {
            sentence += 1;
        }
    }
    if let Some(st) = start {
        out.push(Word { start: st, end: s.len(), lower: s[st..].to_lowercase(), sentence });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_wait_replaces_a_word() {
        assert_eq!(
            apply_self_corrections("Let's meet on Monday, no wait, Tuesday."),
            "Let's meet on Tuesday."
        );
    }

    #[test]
    fn no_wait_cuts_from_a_repeated_word() {
        assert_eq!(
            apply_self_corrections("Let's meet at 3 pm no wait at 4 pm."),
            "Let's meet at 4 pm."
        );
    }

    #[test]
    fn sorry_replaces_the_last_word() {
        assert_eq!(apply_self_corrections("Send it to John, sorry, Jane."), "Send it to Jane.");
    }

    #[test]
    fn i_mean_replaces_a_short_tail() {
        assert_eq!(
            apply_self_corrections("The budget is 5 million, I mean 6 million."),
            "The budget is 6 million."
        );
    }

    #[test]
    fn scratch_that_drops_the_previous_sentence() {
        assert_eq!(
            apply_self_corrections("I'll bring the cake. Scratch that. I'll bring cookies."),
            "I'll bring cookies."
        );
    }

    #[test]
    fn make_that_keeps_the_earlier_words() {
        assert_eq!(
            apply_self_corrections("Buy milk, actually make that oat milk."),
            "Buy oat milk."
        );
    }

    #[test]
    fn i_mean_at_sentence_start_is_untouched() {
        assert_eq!(
            apply_self_corrections("I mean it when I say thanks."),
            "I mean it when I say thanks."
        );
    }

    #[test]
    fn marker_without_leading_words_is_untouched() {
        assert_eq!(apply_self_corrections("No wait is too long."), "No wait is too long.");
    }

    #[test]
    fn keeps_final_punctuation() {
        assert_eq!(apply_self_corrections("Monday, no wait, Tuesday?"), "Tuesday?");
    }

    #[test]
    fn scratch_that_drops_only_the_clause() {
        assert_eq!(
            apply_self_corrections("Buy milk, scratch that, buy bread."),
            "Buy bread."
        );
    }

    #[test]
    fn empty_and_no_change() {
        assert_eq!(apply_self_corrections(""), "");
        assert_eq!(
            apply_self_corrections("Meet me on Tuesday."),
            "Meet me on Tuesday."
        );
    }

    #[test]
    fn unicode_survives() {
        assert_eq!(
            apply_self_corrections("Bring café au lait, no wait, café noir."),
            "Bring café noir."
        );
    }
}
