#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! pakmap library: search backends, install runners, TUI state.

/// Version single-source mirror: the published version lives in the root
/// `[package]` (AGENTS.md's single-source rule) — this const is how OTHER
/// workspace members (pakmap-gui) display it without carrying their own
/// stale crate field around.
pub const PAKMAP_VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod cli;
pub mod config;
pub mod confirm;
pub mod diff;
pub mod error;
pub mod install;
pub mod model;
pub mod news;
pub mod orphans;
pub mod output;
pub mod process;
pub mod safety;
pub mod search;
pub mod sources;
pub mod stats;
pub mod timing;
pub mod tui;
pub mod updates;
