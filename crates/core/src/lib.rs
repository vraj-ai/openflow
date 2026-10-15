//! OpenFlow core: everything that decides *what* happens during dictation,
//! with no knowledge of models, audio devices or operating systems.

pub mod catalog;
pub mod controller;
pub mod desktop;
pub mod dictionary;
pub mod distance;
pub mod engine;
pub mod guard;
pub mod history;
pub mod hotkey;
pub mod mock;
pub mod prompt;
pub mod runtime;
pub mod settings;
pub mod snippets;
pub mod style;
pub mod text;
