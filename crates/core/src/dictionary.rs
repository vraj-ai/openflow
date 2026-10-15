//! Personal dictionary: speech-model hotwords, LLM glossary and a post-replace pass.

use serde::{Deserialize, Serialize};

use crate::distance::normalize_words;

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
