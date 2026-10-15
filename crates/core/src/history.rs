//! Dictation history as an append-only JSONL file, newest last.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::desktop::AppInfo;
use crate::hotkey::Mode;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Unix time in milliseconds.
    pub at_ms: u64,
    pub mode: Mode,
    pub app: AppInfo,
    pub raw: String,
    pub text: String,
    pub used_llm: bool,
    /// Why the LLM output was discarded, if it was.
    #[serde(default)]
    pub rejected: Option<String>,
    pub stt_ms: u64,
    pub cleanup_ms: u64,
}

pub struct History {
    path: PathBuf,
    max_entries: usize,
}

impl History {
    pub fn new(path: PathBuf, max_entries: usize) -> Self {
        Self { path, max_entries }
    }

    pub fn append(&self, entry: &HistoryEntry) -> anyhow::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(&self.path)?;
        writeln!(file, "{}", serde_json::to_string(entry)?)?;
        drop(file);
        let all = self.load()?;
        // Compact only once the file is well past the cap to keep appends cheap.
        if all.len() > self.max_entries * 2 {
            self.rewrite(&all[all.len() - self.max_entries..])?;
        }
        Ok(())
    }

    /// Up to `max_entries` most recent entries, oldest first. Unparseable lines
    /// (a torn write after a crash) are skipped.
    pub fn load(&self) -> anyhow::Result<Vec<HistoryEntry>> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e.into()),
        };
        let entries: Vec<HistoryEntry> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
        let skip = entries.len().saturating_sub(self.max_entries);
        Ok(entries.into_iter().skip(skip).collect())
    }

    pub fn clear(&self) -> anyhow::Result<()> {
        self.rewrite(&[])
    }

    fn rewrite(&self, entries: &[HistoryEntry]) -> anyhow::Result<()> {
        let mut out = String::new();
        for e in entries {
            out.push_str(&serde_json::to_string(e)?);
            out.push('\n');
        }
        let tmp = self.path.with_extension("jsonl.tmp");
        std::fs::write(&tmp, out)?;
        std::fs::rename(tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(n: u64) -> HistoryEntry {
        HistoryEntry {
            at_ms: n,
            mode: Mode::Dictation,
            app: AppInfo::default(),
            raw: format!("raw {n}"),
            text: format!("text {n}"),
            used_llm: false,
            rejected: None,
            stt_ms: 1,
            cleanup_ms: 2,
        }
    }

    #[test]
    fn keeps_most_recent_entries_and_skips_torn_lines() {
        let dir = std::env::temp_dir().join(format!("openflow-history-{}", std::process::id()));
        let h = History::new(dir.join("history.jsonl"), 3);
        for n in 0..8 {
            h.append(&entry(n)).unwrap();
        }
        std::fs::OpenOptions::new().append(true).open(dir.join("history.jsonl")).unwrap().write_all(b"{torn").unwrap();
        let got: Vec<u64> = h.load().unwrap().iter().map(|e| e.at_ms).collect();
        assert_eq!(got, vec![5, 6, 7]);
        h.clear().unwrap();
        assert!(h.load().unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
