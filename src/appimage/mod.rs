//! AppImage integration (v0.12.0): install, integrate, list, remove.
//!
//! No AppImage is ever executed during integration. Metadata is read by
//! `unsquashfs` at the offset parsed from the ELF header, and every file we
//! write is tracked in a manifest so `remove` deletes only our own files.

pub mod command;
mod desktop;
mod download;
mod elf;
mod extract;
pub mod service;
mod store;

pub use elf::{AppImageLayout, inspect};
pub use store::Entry;
