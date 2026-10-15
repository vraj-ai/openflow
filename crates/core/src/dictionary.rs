//! Personal dictionary: speech-model hotwords, LLM glossary and a post-replace pass.

use serde::{Deserialize, Serialize};

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
        todo!()
    }
    pub fn glossary(&self) -> Option<String> {
        todo!()
    }
    pub fn apply(&self, text: &str) -> String {
        todo!("{text}")
    }
    pub fn upsert(&mut self, entry: DictEntry) {
        todo!("{entry:?}")
    }
}
