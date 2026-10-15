//! Accepts or rejects LLM output against the rule-cleaned transcript, so a model
//! that answers, summarises or chats never reaches the user's text field.

use crate::distance::{normalize_words, word_edit_distance};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GuardConfig {
    /// Max word edit distance divided by reference word count.
    pub max_edit_ratio: f64,
    /// Max candidate words as a multiple of reference words (plus `length_slack`).
    pub max_length_ratio: f64,
    pub length_slack: usize,
    /// Min fraction of the reference's content words that must survive.
    pub min_content_recall: f64,
}

impl Default for GuardConfig {
    fn default() -> Self {
        Self { max_edit_ratio: 0.6, max_length_ratio: 1.4, length_slack: 6, min_content_recall: 0.5 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Rejection {
    Empty,
    TooLong { reference_words: usize, candidate_words: usize },
    TooDifferent { edit_ratio: f64 },
    LowRecall { recall: f64 },
}

/// Strips wrapper noise models add (a leading "Here is the cleaned text:",
/// "Cleaned text:", surrounding quotes or code fences, `<think>` blocks), then
/// checks the result. Returns the stripped candidate when accepted.
pub fn check(reference: &str, candidate: &str, cfg: &GuardConfig) -> Result<String, Rejection> {
    let stripped = strip_noise(candidate, reference);
    let cand = normalize_words(&stripped);
    if cand.is_empty() {
        return Err(Rejection::Empty);
    }
    let refs = normalize_words(reference);
    if refs.is_empty() {
        return Ok(stripped);
    }
    if cand.len() as f64 > refs.len() as f64 * cfg.max_length_ratio + cfg.length_slack as f64 {
        return Err(Rejection::TooLong { reference_words: refs.len(), candidate_words: cand.len() });
    }
    let edit_ratio = word_edit_distance(&refs, &cand) as f64 / refs.len().max(1) as f64;
    if edit_ratio > cfg.max_edit_ratio {
        return Err(Rejection::TooDifferent { edit_ratio });
    }
    let content: Vec<&str> = refs
        .iter()
        .filter(|w| w.chars().count() > 3 && !STOPWORDS.contains(&w.as_str()))
        .map(String::as_str)
        .collect();
    if content.len() >= 3 {
        let present = content.iter().filter(|w| cand.iter().any(|c| c.as_str() == **w)).count();
        let recall = present as f64 / content.len() as f64;
        if recall < cfg.min_content_recall {
            return Err(Rejection::LowRecall { recall });
        }
    }
    Ok(stripped)
}

/// Short function words ignored when measuring content recall.
const STOPWORDS: &[&str] = &[
    "the", "a", "an", "and", "or", "but", "if", "then", "than", "of", "to", "in", "on", "at",
    "by", "for", "with", "from", "as", "is", "are", "was", "were", "be", "been", "am", "do",
    "does", "did", "has", "have", "had", "will", "would", "can", "could", "should", "shall",
    "may", "might", "must", "it", "its", "this", "that", "these", "those", "there", "here",
    "what", "when", "where", "which", "who", "whom", "how", "why", "not", "no", "yes", "so",
    "just", "very", "you", "your", "yours", "we", "our", "ours", "i", "me", "my", "mine",
    "he", "him", "his", "she", "her", "hers", "they", "them", "their", "theirs", "us", "them",
];

fn strip_noise(candidate: &str, reference: &str) -> String {
    let s = remove_think_blocks(candidate);
    let s = strip_fence(s.trim());
    let s = strip_preamble(&s);
    let s = s.trim();
    let s = if starts_with_quote(reference.trim_start()) {
        s.to_string()
    } else {
        strip_wrapping_quotes(s)
    };
    s.trim().to_string()
}

fn starts_with_quote(s: &str) -> bool {
    s.starts_with('"') || s.starts_with('\u{201c}')
}

/// Removes `<think>...</think>` spans, case-insensitively; an unterminated span
/// drops the remainder rather than leaking reasoning into the text.
fn remove_think_blocks(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.to_ascii_lowercase().find("<think") {
        out.push_str(&rest[..start]);
        let lower = rest[start..].to_ascii_lowercase();
        let Some(rel) = lower.find("</think") else { return out };
        let after = start + rel;
        match rest[after..].find('>') {
            Some(gt) => rest = &rest[after + gt + 1..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

fn strip_fence(s: &str) -> String {
    let t = s.trim();
    if !t.starts_with("```") {
        return t.to_string();
    }
    let body = match t.find('\n') {
        Some(i) => &t[i + 1..],
        None => return t.trim_matches('`').trim().to_string(),
    };
    let body = body.trim_end();
    let body = body.strip_suffix("```").unwrap_or(body);
    body.trim().to_string()
}

/// Drops a model preamble ("Here is the cleaned text:", "Output:", "Sure,").
fn strip_preamble(s: &str) -> String {
    let t = s.trim_start();
    let lead = s.len() - t.len();
    let line_end = t.find('\n').unwrap_or(t.len());
    let first = &t[..line_end];
    let low = first.to_ascii_lowercase();
    let here = (low.starts_with("here is") || low.starts_with("here's"))
        && ["cleaned", "corrected", "edited", "formatted"].iter().any(|w| low.contains(w))
        && ["text", "version", "transcript"].iter().any(|w| low.contains(w));
    let label = low.starts_with("cleaned text:") || low.starts_with("output:");
    let sure = low == "sure"
        || low.starts_with("sure,")
        || low.starts_with("sure!")
        || low.starts_with("sure.");
    if !(here || label || sure) {
        return t.to_string();
    }
    let cut = match first.find(':') {
        Some(i) => lead + i + 1,
        None => lead + line_end,
    };
    s[cut..].trim_start().to_string()
}

fn strip_wrapping_quotes(s: &str) -> String {
    let t = s.trim();
    if let Some(inner) = t.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        return inner.trim().to_string();
    }
    if let Some(inner) = t.strip_prefix('\u{201c}').and_then(|r| r.strip_suffix('\u{201d}')) {
        return inner.trim().to_string();
    }
    t.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CFG: GuardConfig =
        GuardConfig { max_edit_ratio: 0.6, max_length_ratio: 1.4, length_slack: 6, min_content_recall: 0.5 };

    #[test]
    fn accepts_light_cleanup() {
        let out = check("um so i think we should meet tuesday", "So I think we should meet Tuesday.", &CFG);
        assert_eq!(out, Ok("So I think we should meet Tuesday.".to_string()));
    }

    #[test]
    fn strips_preamble_line() {
        let out = check("hello there", "Here is the cleaned text:\n\nHello there.", &CFG);
        assert_eq!(out, Ok("Hello there.".to_string()));
    }

    #[test]
    fn strips_think_block() {
        let out = check("the answer is forty two", "<think>let me think</think>The answer is forty two.", &CFG);
        assert_eq!(out, Ok("The answer is forty two.".to_string()));
    }

    #[test]
    fn strips_wrapping_quotes() {
        let out = check("hello there", "\"Hello there.\"", &CFG);
        assert_eq!(out, Ok("Hello there.".to_string()));
    }

    #[test]
    fn rejects_blank_candidate() {
        assert_eq!(check("hello there", "   \n", &CFG), Err(Rejection::Empty));
    }

    #[test]
    fn rejects_an_answer_instead_of_a_transcript() {
        let out = check(
            "what is the capital of france",
            "The capital of France is Paris, a city known for the Eiffel Tower and its museums.",
            &CFG,
        );
        assert!(matches!(out, Err(Rejection::TooLong { reference_words: 6, candidate_words: 16 })), "{out:?}");
    }

    #[test]
    fn rejects_a_summary_that_drops_content() {
        let out = check(
            "review the quarterly budget report and rewrite the summary",
            "review the monthly spending notes and redo the summary",
            &CFG,
        );
        assert!(matches!(out, Err(Rejection::LowRecall { .. })), "{out:?}");
    }

    #[test]
    fn rejects_an_unrelated_rewrite() {
        let out = check("hello world", "goodbye moon", &CFG);
        assert!(matches!(out, Err(Rejection::TooDifferent { .. })), "{out:?}");
    }
}
