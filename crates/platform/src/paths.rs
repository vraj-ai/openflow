//! Per-user locations. `OPENFLOW_HOME` overrides everything (tests, portable installs).

use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("OPENFLOW_HOME") {
        return PathBuf::from(home);
    }
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("OpenFlow")
}

pub fn settings_file() -> PathBuf {
    data_dir().join("settings.json")
}

pub fn history_file() -> PathBuf {
    data_dir().join("history.jsonl")
}
