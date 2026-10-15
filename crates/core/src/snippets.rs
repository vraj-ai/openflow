//! Snippets expand a spoken cue ("my calendar link") into saved text.

use serde::{Deserialize, Serialize};

use crate::distance::normalize_words;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snippet {
    pub cue: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Snippets {
    pub items: Vec<Snippet>,
}

impl Snippets {
    /// Whole-utterance match on normalized words, optionally prefixed with
    /// "insert" / "paste" / "type".
    pub fn whole_match(&self, utterance: &str) -> Option<&Snippet> {
        let words = normalize_words(utterance);
        self.items.iter().find(|s| {
            let cue = normalize_words(&s.cue);
            if cue.is_empty() {
                return false;
            }
            if words == cue {
                return true;
            }
            words.len() == cue.len() + 1
                && matches!(words[0].as_str(), "insert" | "paste" | "type")
                && words[1..] == cue[..]
        })
    }

    /// Replaces every cue occurrence in `text`, longest cues first. Punctuation
    /// between cue words is ignored; trailing punctuation is preserved.
    pub fn expand(&self, text: &str) -> String {
        let cues: Vec<(Vec<String>, &str)> = self
            .items
            .iter()
            .filter_map(|s| {
                let words = normalize_words(&s.cue);
                (!words.is_empty()).then_some((words, s.text.as_str()))
            })
            .collect();
        let mut order: Vec<usize> = (0..cues.len()).collect();
        order.sort_by_key(|&k| std::cmp::Reverse(cues[k].0.len()));
        let segs = tokenize(text);
        let mut out = String::new();
        let mut i = 0;
        while i < segs.len() {
            let mut used = 0usize;
            let mut replacement: Option<&str> = None;
            if word_at(&segs, i).is_some() {
                for &k in &order {
                    let (cue, text) = &cues[k];
                    if seq_matches(&segs, i, cue) {
                        used = cue.len();
                        replacement = Some(*text);
                        break;
                    }
                }
            }
            match replacement {
                Some(text) => {
                    out.push_str(text);
                    i += 2 * (used - 1) + 1;
                }
                None => {
                    match &segs[i] {
                        Seg::Word(w) => out.push_str(w),
                        Seg::Sep(s) => out.push_str(s),
                    }
                    i += 1;
                }
            }
        }
        out
    }

    pub fn cues_in(&self, text: &str) -> Vec<&Snippet> {
        let segs = tokenize(text);
        self.items
            .iter()
            .filter(|s| {
                let cue = normalize_words(&s.cue);
                !cue.is_empty() && (0..segs.len()).any(|i| seq_matches(&segs, i, &cue))
            })
            .collect()
    }
}

/// A maximal run of word characters or of separators. Words in a segment list are
/// always separated by exactly one separator segment, so the next word after index
/// `i` sits at `i + 2`.
enum Seg {
    Word(String),
    Sep(String),
}

fn tokenize(text: &str) -> Vec<Seg> {
    let mut segs = Vec::new();
    let mut cur = String::new();
    let mut word = false;
    for ch in text.chars() {
        let w = ch.is_alphanumeric() || ch == '\'';
        if !cur.is_empty() && w != word {
            segs.push(if word { Seg::Word(std::mem::take(&mut cur)) } else { Seg::Sep(std::mem::take(&mut cur)) });
        }
        cur.push(ch);
        word = w;
    }
    if !cur.is_empty() {
        segs.push(if word { Seg::Word(cur) } else { Seg::Sep(cur) });
    }
    segs
}

fn word_at(segs: &[Seg], i: usize) -> Option<&str> {
    match segs.get(i) {
        Some(Seg::Word(w)) => Some(w),
        _ => None,
    }
}

fn seq_matches(segs: &[Seg], i: usize, cue: &[String]) -> bool {
    cue.iter().enumerate().all(|(k, want)| {
        word_at(segs, i + 2 * k).map(|w| w.to_lowercase() == *want).unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snippets() -> Snippets {
        // Shorter cue deliberately first, so expansions must pick by length.
        Snippets {
            items: vec![
                Snippet { cue: "calendar link".into(), text: "https://cal.com".into() },
                Snippet { cue: "my calendar link".into(), text: "https://cal.com/v".into() },
            ],
        }
    }

    #[test]
    fn whole_match_ignores_case_and_trailing_punctuation() {
        let s = snippets();
        assert_eq!(s.whole_match("My calendar link!").map(|x| x.text.as_str()), Some("https://cal.com/v"));
        assert_eq!(
            s.whole_match("Insert my calendar link.").map(|x| x.text.as_str()),
            Some("https://cal.com/v")
        );
    }

    #[test]
    fn whole_match_rejects_partial_cues() {
        let s = snippets();
        assert_eq!(s.whole_match("my calendar"), None);
        assert_eq!(s.whole_match("add my calendar link to the invite"), None);
    }

    #[test]
    fn expand_replaces_mid_sentence_and_keeps_punctuation() {
        let s = snippets();
        assert_eq!(s.expand("Book time at my calendar link, thanks."), "Book time at https://cal.com/v, thanks.");
    }

    #[test]
    fn expand_prefers_the_longest_cue() {
        let s = snippets();
        assert_eq!(s.expand("my calendar link"), "https://cal.com/v");
    }

    #[test]
    fn expand_does_not_match_inside_words() {
        let s = Snippets { items: vec![Snippet { cue: "cat".into(), text: "feline".into() }] };
        assert_eq!(s.expand("cat and cats"), "feline and cats");
        assert!(s.cues_in("a catalog").is_empty());
    }

    #[test]
    fn cues_in_reports_present_cues() {
        let s = snippets();
        let found: Vec<&str> = s.cues_in("Book time at my calendar link").iter().map(|x| x.cue.as_str()).collect();
        assert_eq!(found, vec!["calendar link", "my calendar link"]);
    }
}
