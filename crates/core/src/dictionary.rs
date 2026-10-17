//! Personal dictionary: speech-model hotwords, LLM glossary and a post-replace pass.

use serde::{Deserialize, Serialize};

use crate::distance::{normalize_words, word_edit_distance};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntrySource {
    #[default]
    Manual,
    Learned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DictEntry {
    /// Canonical spelling, e.g. "Kubernetes" or "OpenFlow".
    pub term: String,
    /// Phrases the speech model produces instead, e.g. ["cooper netties"].
    #[serde(default)]
    pub sounds_like: Vec<String>,
    #[serde(default)]
    pub source: EntrySource,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Dictionary {
    pub entries: Vec<DictEntry>,
}

impl Dictionary {
    pub fn hotwords(&self) -> Vec<String> {
        let mut seen = Vec::new();
        let mut out = Vec::new();
        for e in &self.entries {
            if !seen.iter().any(|s: &String| s.eq_ignore_ascii_case(&e.term)) {
                seen.push(e.term.clone());
                out.push(e.term.clone());
            }
        }
        out
    }

    pub fn glossary(&self) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        Some(self.entries.iter().map(|e| e.term.as_str()).collect::<Vec<_>>().join(", "))
    }

    pub fn apply(&self, text: &str) -> String {
        // (term lowercased, canonical term) — longest-match-first is applied below.
        let mut terms: Vec<(String, &str)> = Vec::new();
        let mut sounds: Vec<(Vec<String>, &str)> = Vec::new();
        for e in &self.entries {
            terms.push((e.term.trim().to_lowercase(), e.term.as_str()));
            for phrase in &e.sounds_like {
                let words = normalize_words(phrase);
                if !words.is_empty() {
                    sounds.push((words, e.term.as_str()));
                }
            }
        }
        sounds.sort_by_key(|(w, _)| std::cmp::Reverse(w.len()));

        let segs = tokenize(text);
        let mut out = String::new();
        let mut i = 0;
        while i < segs.len() {
            let mut used = 0usize;
            let mut replacement: Option<&str> = None;
            if let Some(word0) = word_at(&segs, i) {
                // (a) sounds-like phrases, longest first.
                for (phrase, term) in &sounds {
                    if seq_matches(&segs, i, phrase) {
                        used = phrase.len();
                        replacement = Some(*term);
                        break;
                    }
                }
                // (c) join 3 then 2 adjacent words into a single-word term.
                if replacement.is_none() {
                    'join: for n in [3usize, 2] {
                        if (0..n).any(|k| word_at(&segs, i + 2 * k).is_none()) {
                            continue;
                        }
                        let joined: String =
                            (0..n).map(|k| word_at(&segs, i + 2 * k).unwrap().to_lowercase()).collect();
                        let matched = if joined.chars().count() >= 5 {
                            terms.iter().find(|(t, _)| !t.contains(' ') && *t == joined)
                        } else {
                            None
                        };
                        if let Some((_, term)) = matched {
                            used = n;
                            replacement = Some(*term);
                            break 'join;
                        }
                    }
                }
                // (b) case-normalize a lone term.
                if replacement.is_none() {
                    let lower = word0.to_lowercase();
                    if let Some((_, term)) = terms.iter().find(|(t, _)| *t == lower) {
                        used = 1;
                        replacement = Some(*term);
                    }
                }
            }
            match replacement {
                Some(term) => {
                    out.push_str(term);
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

    pub fn upsert(&mut self, entry: DictEntry) {
        if let Some(existing) = self.entries.iter_mut().find(|e| e.term.eq_ignore_ascii_case(&entry.term))
        {
            for phrase in entry.sounds_like {
                if !existing.sounds_like.iter().any(|s| s.eq_ignore_ascii_case(&phrase)) {
                    existing.sounds_like.push(phrase);
                }
            }
            if entry.source == EntrySource::Manual {
                existing.source = EntrySource::Manual;
            }
        } else {
            self.entries.push(entry);
        }
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

fn seq_matches(segs: &[Seg], i: usize, phrase: &[String]) -> bool {
    phrase.iter().enumerate().all(|(k, want)| {
        word_at(segs, i + 2 * k).map(|w| w.to_lowercase() == *want).unwrap_or(false)
    })
}

/// The words the user corrected in a text field after we pasted.
///
/// The field usually holds more than our text, so the region that best aligns
/// with what we inserted is the one that gets diffed.
pub fn learn_from_field(inserted: &str, field: &str) -> Vec<DictEntry> {
    /// The window search is cubic in the field, so only the tail of a long
    /// document is compared. A correction further up a very long file is not
    /// learned.
    // ponytail: tail-only scan; anchor on the inserted text's first rare word
    // if learning inside long documents ever matters.
    const MAX_FIELD_WORDS: usize = 400;
    let ins = normalize_words(inserted);
    let words = normalize_words(field);
    if ins.is_empty() || words.is_empty() || words == ins {
        return Vec::new();
    }
    let words = if words.len() > MAX_FIELD_WORDS {
        &words[words.len() - MAX_FIELD_WORDS..]
    } else {
        &words[..]
    };
    let mut best: Option<(usize, usize, usize)> = None;
    // Longest window first, so a tie on distance keeps the words that changed
    // instead of cutting them off at the window edge.
    for len in (3..=ins.len() + 3).rev() {
        if len > words.len() {
            continue;
        }
        for start in 0..=words.len() - len {
            let d = word_edit_distance(&ins, &words[start..start + len]);
            if best.is_none_or(|(bd, _, _)| d < bd) {
                best = Some((d, start, len));
            }
        }
    }
    match best {
        Some((_, start, len)) => learn_from_edit(inserted, &words[start..start + len].join(" ")),
        None => Vec::new(),
    }
}

/// Words the user corrected in our own output, as dictionary entries: the
/// edited phrase is the term, the words it replaced are what the model heard.
///
/// Only small, local edits teach anything: a rewrite larger than a third of the
/// text (or a phrase longer than three words) is treated as new writing, not a
/// spelling correction.
pub fn learn_from_edit(original: &str, edited: &str) -> Vec<DictEntry> {
    const MIN_WORD: usize = 4;
    const MAX_TERM_WORDS: usize = 3;
    let a = normalize_words(original);
    let b = normalize_words(edited);
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let ops = align(&a, &b);
    // A hunk is one run of replaced words and what replaced it.
    let mut hunks: Vec<(Vec<String>, Vec<String>)> = Vec::new();
    let mut i = 0;
    while i < ops.len() {
        if matches!(ops[i], Op::Same) {
            i += 1;
            continue;
        }
        let (mut replaced, mut terms) = (Vec::new(), Vec::new());
        while i < ops.len() {
            match &ops[i] {
                Op::Delete(k) => replaced.push(a[*k].clone()),
                Op::Insert(k) => terms.push(b[*k].clone()),
                Op::Same => break,
            }
            i += 1;
        }
        hunks.push((replaced, terms));
    }
    let changed: usize = hunks.iter().map(|(r, t)| r.len() + t.len()).sum();
    if hunks.is_empty() || changed > (a.len().max(b.len()) / 3).max(4) {
        return Vec::new();
    }
    let mut out: Vec<DictEntry> = Vec::new();
    for (replaced, terms) in hunks {
        if replaced.is_empty() || terms.is_empty() {
            continue;
        }
        let term = terms.join(" ");
        let heard = replaced.join(" ");
        if terms.len() <= MAX_TERM_WORDS
            && term.chars().count() >= MIN_WORD
            && !is_common(&term)
            && !replaced.iter().any(|w| w == &term)
            && !out.iter().any(|e| e.term.eq_ignore_ascii_case(&term))
        {
            out.push(DictEntry { term, sounds_like: vec![heard], source: EntrySource::Learned });
        }
    }
    out
}

/// Words too common to be worth teaching the speech model.
fn is_common(text: &str) -> bool {
    const COMMON: [&str; 24] = [
        "that", "this", "with", "have", "from", "they", "there", "their", "them", "then", "than", "when",
        "what", "will", "would", "could", "should", "about", "because", "which", "your", "just", "like",
        "some",
    ];
    let lower = text.to_lowercase();
    lower.split_whitespace().any(|w| COMMON.contains(&w))
}

enum Op {
    Same,
    Delete(usize),
    Insert(usize),
}

/// Alignment of two word lists: equal positions, deletions from `a`, insertions
/// into `b`, from the longest common subsequence.
fn align(a: &[String], b: &[String]) -> Vec<Op> {
    let mut lcs = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut ops = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            ops.push(Op::Same);
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            ops.push(Op::Delete(i));
            i += 1;
        } else {
            ops.push(Op::Insert(j));
            j += 1;
        }
    }
    while i < a.len() {
        ops.push(Op::Delete(i));
        i += 1;
    }
    while j < b.len() {
        ops.push(Op::Insert(j));
        j += 1;
    }
    ops
}

#[cfg(test)]
mod learn_tests {
    use super::*;

    #[test]
    fn learns_a_corrected_word() {
        let got = learn_from_edit("I deployed it to cooper netties", "I deployed it to Kubernetes");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].term, "kubernetes");
        assert_eq!(got[0].sounds_like, vec!["cooper netties".to_string()]);
        assert_eq!(got[0].source, EntrySource::Learned);
    }

    #[test]
    fn learns_a_multi_word_term() {
        let got = learn_from_edit("book a room at the grand hyatt", "book a room at the Grand Hyatt");
        assert!(got.is_empty(), "case-only edits teach nothing: {got:?}");
        let got = learn_from_edit("we met at the coffee house", "we met at the Blue Bottle");
        assert_eq!(got[0].term, "blue bottle");
        assert_eq!(got[0].sounds_like, vec!["coffee house".to_string()]);
    }

    #[test]
    fn ignores_unchanged_text_and_big_rewrites() {
        assert!(learn_from_edit("send the deck on friday", "send the deck on friday").is_empty());
        assert!(learn_from_edit(
            "send the deck on friday",
            "let us move the whole thing to next week instead"
        )
        .is_empty());
    }

    #[test]
    fn ignores_short_and_common_words() {
        assert!(learn_from_edit("i said ok", "i said yes").is_empty());
        assert!(learn_from_edit("i went there", "i went with").is_empty());
    }

    #[test]
    fn learns_when_the_field_is_exactly_the_corrected_text() {
        let got = learn_from_field("I deployed it to cooper netties", "I deployed it to Kubernetes");
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].term, "kubernetes");
    }

    /// A long document must not turn the cubic window search into a stall on the
    /// event loop, and a correction inside the tail is still learned.
    #[test]
    fn a_long_field_is_scanned_at_its_tail() {
        let filler = "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod";
        let mut field = filler.repeat(100); // ~1000 words
        let start = std::time::Instant::now();
        assert!(learn_from_field("we shipped the deck", &field).is_empty());
        assert!(start.elapsed() < std::time::Duration::from_secs(5), "scan took {:?}", start.elapsed());

        field.push_str(" we shipped the dock instead");
        let got = learn_from_field("we shipped the deck", &field);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].term, "dock");
        assert_eq!(got[0].sounds_like, vec!["deck".to_string()]);
    }

    #[test]
    fn finds_the_inserted_region_inside_a_longer_field() {
        let field = "Hi Jane, I deployed it to Kubernetes yesterday, thanks";
        let got = learn_from_field("I deployed it to cooper netties yesterday", field);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].term, "kubernetes");
        assert_eq!(got[0].sounds_like, vec!["cooper netties".to_string()]);
        assert!(learn_from_field("I deployed it to Kubernetes", field).is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(term: &str, sounds_like: &[&str], source: EntrySource) -> DictEntry {
        DictEntry {
            term: term.to_string(),
            sounds_like: sounds_like.iter().map(|s| s.to_string()).collect(),
            source,
        }
    }

    #[test]
    fn hotwords_and_glossary_list_terms_in_order() {
        let d = Dictionary {
            entries: vec![entry("Kubernetes", &[], EntrySource::Manual), entry("OpenFlow", &[], EntrySource::Learned)],
        };
        assert_eq!(d.hotwords(), vec!["Kubernetes", "OpenFlow"]);
        assert_eq!(d.glossary().as_deref(), Some("Kubernetes, OpenFlow"));
        assert_eq!(Dictionary::default().glossary(), None);
    }

    #[test]
    fn hotwords_dedups_case_insensitively() {
        let d = Dictionary {
            entries: vec![entry("GitHub", &[], EntrySource::Manual), entry("github", &[], EntrySource::Learned)],
        };
        assert_eq!(d.hotwords(), vec!["GitHub"]);
    }

    #[test]
    fn replaces_sounds_like_phrase_keeping_punctuation() {
        let d = Dictionary { entries: vec![entry("Kubernetes", &["cooper netties"], EntrySource::Manual)] };
        assert_eq!(d.apply("we run cooper netties, in prod"), "we run Kubernetes, in prod");
    }

    #[test]
    fn case_normalizes_whole_words_only() {
        let d = Dictionary { entries: vec![entry("GitHub", &[], EntrySource::Manual)] };
        assert_eq!(d.apply("see github docs"), "see GitHub docs");
        assert_eq!(d.apply("a githubber wrote this"), "a githubber wrote this");
    }

    #[test]
    fn joins_split_words() {
        let d = Dictionary {
            entries: vec![
                entry("OpenFlow", &[], EntrySource::Manual),
                entry("PostGres", &[], EntrySource::Manual),
                entry("PostgreSQL", &[], EntrySource::Manual),
            ],
        };
        assert_eq!(d.apply("open flow rocks"), "OpenFlow rocks");
        assert_eq!(d.apply("post gres is fine"), "PostGres is fine");
        assert_eq!(d.apply("post gres ql here"), "PostgreSQL here");
    }

    #[test]
    fn upsert_merges_sounds_like_and_prefers_manual() {
        let mut d = Dictionary { entries: vec![entry("Kubernetes", &["cooper netties"], EntrySource::Learned)] };
        d.upsert(entry("kubernetes", &["koober netties", "COOPER NETTIES"], EntrySource::Manual));
        assert_eq!(d.entries.len(), 1);
        assert_eq!(d.entries[0].sounds_like, vec!["cooper netties", "koober netties"]);
        assert_eq!(d.entries[0].source, EntrySource::Manual);
    }

    #[test]
    fn upsert_appends_new_term() {
        let mut d = Dictionary::default();
        d.upsert(entry("OpenFlow", &[], EntrySource::Manual));
        assert_eq!(d.entries.len(), 1);
        assert_eq!(d.entries[0].term, "OpenFlow");
    }
}
