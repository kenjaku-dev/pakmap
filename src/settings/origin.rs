//! Origins: where an effective setting value came from, and the layer model.
//!
//! Layer order (plan §1.3, D4): defaults < user file < project file (allowed
//! sections only) < env < CLI flags. The env/CLI layers keep applying at the
//! op layer as before; `resolve()` only annotates their existence — file
//! origins below are the layer this engine owns.

use std::path::PathBuf;

/// Which file layer accepted/disallowed a key (plan §1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Default,
    User,
    Project,
    Env,
    Cli,
}

/// Where a setting's effective value originates (plan §1.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// Built-in default (no file set the key).
    Default,
    /// Set in the user file (XDG or `--config PATH`).
    UserFile(PathBuf),
    /// Set in `./pakmap.toml` (allowed sections only, plan D2/D4).
    ProjectFile(PathBuf),
    /// Session environment (annotated; op layer applies).
    Env,
    /// CLI flag (annotated; op layer applies).
    Cli,
    /// Decided by the device (gui.theme = "device" per plan D1).
    Device,
}

impl Origin {
    /// Stable lowercase token for `--json` output.
    pub fn as_label(&self) -> &'static str {
        match self {
            Origin::Default => "default",
            Origin::UserFile(_) => "user",
            Origin::ProjectFile(_) => "project",
            Origin::Env => "env",
            Origin::Cli => "cli",
            Origin::Device => "device",
        }
    }

    /// Human column string for `config list` (`short` trims " file").
    pub fn render(&self, short: bool) -> String {
        let path = match self {
            Origin::UserFile(p) | Origin::ProjectFile(p) => Some(p),
            _ => None,
        };
        match (self.as_label(), path, short) {
            ("default" | "env" | "cli" | "device", _, _) => self.as_label().to_string(),
            ("user", _, true) => "user".into(),
            ("project", _, true) => "project".into(),
            ("user", Some(p), false) => format!("user ({})", p.display()),
            ("project", Some(p), false) => format!("project ({})", p.display()),
            (_, _, _) => self.as_label().to_string(),
        }
    }
}

/// Parse a layer token (echo back origins for display/tests).
pub fn layer_from_label(label: &str) -> Option<Layer> {
    match label {
        "default" => Some(Layer::Default),
        "user" => Some(Layer::User),
        "project" => Some(Layer::Project),
        "env" => Some(Layer::Env),
        "cli" => Some(Layer::Cli),
        _ => None,
    }
}
