//! Shared safety primitives (v0.9.3 security hardening — plan Phase 1).
//!
//! One implementation serving the CLI surface AND `pakmap-gui` (which consumes
//! `pakmap::safety::…`), so a single policy cannot drift between the two
//! binaries. std-only: no new dependencies, MSRV-safe, and every function is
//! panic-free on hostile input (the workspace denies `unwrap`/`expect`).
//!
//! Phase 1 ships these primitives unwired (tests only); Phase 2 routes the
//! core call sites through them, one commit, fully revertable.

pub mod fs;
pub mod names;
pub mod text;
pub mod url;
