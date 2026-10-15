//! Word-level metrics shared by the guardrail and the eval harness.

/// Lowercased words with surrounding punctuation stripped (inner apostrophes and
/// hyphens kept). "Hello, World!" -> ["hello", "world"].
pub fn normalize_words(text: &str) -> Vec<String> {
    todo!("{text}")
}

/// Levenshtein distance over word tokens.
pub fn word_edit_distance<S: AsRef<str>>(a: &[S], b: &[S]) -> usize {
    todo!("{} {}", a.len(), b.len())
}

/// Word error rate of `hypothesis` against `reference` after `normalize_words`.
/// 0.0 when both are empty; 1.0 when only the reference is empty.
pub fn wer(reference: &str, hypothesis: &str) -> f64 {
    todo!("{reference} {hypothesis}")
}

/// Character error rate on lowercased text with whitespace collapsed.
pub fn cer(reference: &str, hypothesis: &str) -> f64 {
    todo!("{reference} {hypothesis}")
}
