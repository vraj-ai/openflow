//! Deterministic cleanup rules that run with or without the LLM.

pub mod corrections;
pub mod fillers;
pub mod lists;
pub mod spoken;
pub mod tidy;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct RuleSet {
    pub fillers: bool,
    pub self_corrections: bool,
    pub lists: bool,
    pub spoken_commands: bool,
}

impl Default for RuleSet {
    fn default() -> Self {
        Self { fillers: true, self_corrections: true, lists: true, spoken_commands: true }
    }
}

/// Runs the enabled rules in a fixed order: fillers, self-corrections,
/// spoken commands, lists, then `tidy::tidy` always.
pub fn apply_rules(text: &str, rules: &RuleSet) -> String {
    let mut out = text.to_string();
    if rules.fillers {
        out = fillers::remove_fillers(&out);
    }
    if rules.self_corrections {
        out = corrections::apply_self_corrections(&out);
    }
    if rules.spoken_commands {
        out = spoken::apply_spoken_commands(&out);
    }
    if rules.lists {
        out = lists::format_lists(&out);
    }
    tidy::tidy(&out)
}
