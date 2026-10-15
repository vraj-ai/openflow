//! Removes hesitation tokens and immediate stutters.
//!
//! "like", "you know" and "I mean" are deliberately left alone: they are
//! ambiguous enough that the LLM cleanup beats a rule.

use super::tidy;

const FILLERS: [&str; 9] = ["um", "umm", "uh", "uhh", "er", "erm", "ah", "hmm", "mm"];
/// Short function words where a repeated pair collapses to one ("the the").
/// "that that" and "had had" are deliberately absent.
const STUTTERS: [&str; 13] =
    ["i", "the", "a", "an", "to", "and", "we", "it", "so", "but", "of", "in", "is"];

pub fn remove_fillers(text: &str) -> String {
    let text = remove_filler_words(text);
    let text = collapse_stutters(&text);
    let text = text.trim_matches(|c: char| c == ',' || c == ';' || c.is_whitespace());
    tidy::tidy(text)
}

/// Deletes each filler word together with an adjacent comma (or a sentence
/// period), but only when the word is surrounded by punctuation or the text
/// edges, so "5 mm" and "so um no" keep their words.
fn remove_filler_words(s: &str) -> String {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for &(start, end) in &words(s) {
        let w = s[start..end].to_lowercase();
        if !FILLERS.contains(&w.as_str()) {
            continue;
        }
        if !is_boundary(s[..start].trim_end().chars().next_back())
            || !is_boundary(s[end..].trim_start().chars().next())
        {
            continue;
        }
        let mut a = start;
        let mut b = end;
        let mut i = b;
        while s[i..].starts_with(' ') {
            i += 1;
        }
        if let Some(',' | '.') = s[i..].chars().next() {
            b = i + 1;
        }
        let mut j = a;
        while j > 0 && s[..j].ends_with(' ') {
            j -= 1;
        }
        if j > 0 && s[..j].ends_with(',') {
            a = j - 1;
        }
        ranges.push((a, b));
    }
    remove_ranges(s, ranges)
}

fn is_boundary(c: Option<char>) -> bool {
    match c {
        None => true,
        Some(c) => matches!(c, ',' | '.' | '!' | '?' | ';' | ':' | '"' | '\'' | '(' | ')' | '…'),
    }
}

fn collapse_stutters(text: &str) -> String {
    let mut s = text.to_string();
    loop {
        let ws = words(&s);
        let mut hit = None;
        for k in 0..ws.len().saturating_sub(1) {
            let (a0, a1) = ws[k];
            let (b0, b1) = ws[k + 1];
            let w = s[a0..a1].to_lowercase();
            if w == s[b0..b1].to_lowercase()
                && STUTTERS.contains(&w.as_str())
                && s[a1..b0].chars().all(|c| c == ' ' || c == '\t')
            {
                hit = Some((a1, b1));
                break;
            }
        }
        match hit {
            Some((a, b)) => s = format!("{}{}", &s[..a], &s[b..]),
            None => return s,
        }
    }
}

fn remove_ranges(s: &str, mut ranges: Vec<(usize, usize)>) -> String {
    if ranges.is_empty() {
        return s.to_string();
    }
    ranges.sort();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (a, b) in ranges {
        match merged.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    let mut out = String::with_capacity(s.len());
    let mut pos = 0;
    for (a, b) in merged {
        out.push_str(&s[pos..a]);
        pos = b;
    }
    out.push_str(&s[pos..]);
    out
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '-'
}

fn words(s: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in s.char_indices() {
        if is_word_char(c) {
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
    fn removes_leading_and_inner_fillers() {
        assert_eq!(remove_fillers("Um, so I think we should, uh, meet."), "So I think we should meet.");
    }

    #[test]
    fn uh_huh_is_not_a_filler() {
        assert_eq!(remove_fillers("Uh-huh, sounds good."), "Uh-huh, sounds good.");
    }

    #[test]
    fn filler_only_sentence_disappears() {
        assert_eq!(remove_fillers("Hmm."), "");
        assert_eq!(remove_fillers("Uh, um, hmm."), "");
    }

    #[test]
    fn collapses_stutter_pairs() {
        assert_eq!(remove_fillers("I I think the the plan is fine."), "I think the plan is fine.");
        assert_eq!(remove_fillers("We we can to to go."), "We can to go.");
    }

    #[test]
    fn keeps_unlisted_repeats() {
        assert_eq!(remove_fillers("That that is what I said."), "That that is what I said.");
        assert_eq!(remove_fillers("We had had lunch."), "We had had lunch.");
    }

    #[test]
    fn keeps_ambiguous_phrases() {
        assert_eq!(remove_fillers("You know, I mean, like this."), "You know, I mean, like this.");
    }

    #[test]
    fn keeps_measure_units() {
        assert_eq!(remove_fillers("Cut it to 5 mm wide."), "Cut it to 5 mm wide.");
    }

    #[test]
    fn no_double_spaces_or_leading_comma() {
        assert_eq!(remove_fillers("Uh, um, hello."), "Hello.");
    }

    #[test]
    fn empty_and_no_change() {
        assert_eq!(remove_fillers(""), "");
        assert_eq!(remove_fillers("This is a normal sentence."), "This is a normal sentence.");
    }

    #[test]
    fn unicode_around_a_filler() {
        assert_eq!(remove_fillers("Über, um, alles."), "Über alles.");
    }
}
