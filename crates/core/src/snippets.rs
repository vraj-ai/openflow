//! Snippets expand a spoken cue ("my calendar link") into saved text.

use serde::{Deserialize, Serialize};

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
    pub fn whole_match(&self, utterance: &str) -> Option<&Snippet> {
        todo!("{utterance}")
    }
    pub fn expand(&self, text: &str) -> String {
        todo!("{text}")
    }
    pub fn cues_in(&self, text: &str) -> Vec<&Snippet> {
        todo!("{text}")
    }
}
