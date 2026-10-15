//! Accepts or rejects LLM output against the rule-cleaned transcript, so a model
//! that answers, summarises or chats never reaches the user's text field.

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
    todo!("{reference} {candidate} {cfg:?}")
}
