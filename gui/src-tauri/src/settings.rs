//! Settings IPC (plan §3): effective-config read + per-key write/unset.
//!
//! ONE read command returns the whole catalog surface (value, default, kind,
//! origin, apply tag, danger, description — plus global warnings), rendered
//! from the living engine on every call. Unlike the hand-maintained
//! `ConfigSummary` mirror, a new engine key surfaces here automatically: the
//! catalog is the contract.
//!
//! Writes go through `pakmap::settings::write` — the same doors the CLI
//! subcommand uses, so the refusal taxonomy (unknown-key / invalid-value /
//! unparseable / conflict / policy-refused), the fingerprint conflict gate,
//! the atomic rename and the `PAKMAP_TRUST_PROJECT` policy are identical by
//! construction, not by re-implementation.
//!
//! `get_config_flags` is NOT re-routed here: it stays byte-identical as the
//! smoke harness's first-IPC probe (`docs/gui-architecture.md` §runtime
//! budgets) and this module deliberately does not import that path.

use serde::Serialize;

use pakmap::config::Config;
use pakmap::settings::catalog::{CATALOG, ENV_ROWS, Kind, SettingMeta};
use pakmap::settings::origin::Origin;
use pakmap::settings::validate::validate as validate_value;
use pakmap::settings::write;
use pakmap::settings::write::{SetError, Target};

use crate::GuiError;

// ---------------------------------------------------------------------------
// Wire types — snake_case, mirrored 1:1 in gui/src/types.ts
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[cfg_attr(test, derive(Clone))]
pub struct SettingsSurfaces {
    pub cli: bool,
    pub tui: bool,
    pub gui: bool,
}

/// Origin of the effective value, as label + optional file path (the GUI
/// chip renders the label; the path feeds the deep explanation).
#[derive(Serialize)]
#[cfg_attr(test, derive(Clone))]
pub struct SettingsOrigin {
    /// `default` | `user` | `project` | `env` | `cli` | `device`
    pub label: &'static str,
    pub path: Option<String>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(Clone))]
pub struct SettingsEnvRow {
    pub name: &'static str,
    pub desc: &'static str,
}

/// One allowed value of an `enum` row — rendered from the engine's `EnumVal`
/// (that type carries no serde derive on purpose, so the wire gets its own).
#[derive(Serialize)]
#[cfg_attr(test, derive(Clone))]
pub struct SettingsEnumOption {
    pub name: &'static str,
    pub desc: &'static str,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(Clone))]
pub struct SettingsEntry {
    pub key: &'static str,
    /// Effective value, kind-rendered string (`None` options render "").
    pub value: String,
    pub default: &'static str,
    /// `bool` | `u16` | `u32` | `u64` | `usize` | `enum` | `style` |
    /// `text` | `opt-text` | `pair`
    pub kind: &'static str,
    /// Allowed values for `kind = "enum"` (empty otherwise).
    pub options: Vec<SettingsEnumOption>,
    pub group: &'static str,
    /// `""` | `caution` | `danger`
    pub danger: &'static str,
    pub surfaces: SettingsSurfaces,
    /// `immediate` | `next-launch` | `live-preview`
    pub apply: &'static str,
    pub desc: &'static str,
    pub origin: SettingsOrigin,
    /// file-layer warning targeting exactly this key (unknown/invalid),
    /// fail-open: the row stays visible and editable regardless.
    pub warning: Option<String>,
    /// false = the effective value fails its own kind's validation (the
    /// row's inline repair message replaces the control with a reset hint).
    pub valid: bool,
    /// inclusive engine clamp `(min, max)` for numeric kinds (plan §3
    /// badges) — `Some` only where `validate::numeric_clamp` documents one.
    pub clamp: Option<(u64, u64)>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(Clone))]
pub struct EffectiveConfig {
    pub app_version: &'static str,
    pub entries: Vec<SettingsEntry>,
    pub env_rows: Vec<SettingsEnvRow>,
    /// engine warnings (unknown keys, [behavior] strips, invalid config …).
    pub warnings: Vec<String>,
    pub user_path: Option<String>,
    pub project_path: Option<String>,
    pub project_trusted: bool,
}

// ---------------------------------------------------------------------------
// Read
// ---------------------------------------------------------------------------

/// Whole-catalog read: engine truth for every key + where it came from.
#[tauri::command]
pub(crate) fn get_effective_config() -> EffectiveConfig {
    let resolved = Config::load_resolved();
    let cfg = resolved.config;
    let mut warnings = resolved.warnings.clone();
    if !resolved.aliases.is_empty() {
        warnings.push(
            "note: deprecated alias [behavior] bottom_up — use [search] bottom_up".to_string(),
        );
    }
    let entries = CATALOG
        .iter()
        .map(|meta| render_entry(meta, &resolved.origins, &meta.field.get(&cfg)))
        .collect();
    EffectiveConfig {
        app_version: pakmap::PAKMAP_VERSION,
        entries,
        env_rows: ENV_ROWS
            .iter()
            .map(|r| SettingsEnvRow {
                name: r.name,
                desc: r.desc,
            })
            .collect(),
        warnings,
        user_path: resolved.user_path.as_ref().map(|p| p.display().to_string()),
        project_path: resolved
            .project_path
            .as_ref()
            .map(|p| p.display().to_string()),
        project_trusted: resolved.project_trusted,
    }
}

fn render_entry(meta: &SettingMeta, origins: &[(String, Origin)], value: &str) -> SettingsEntry {
    let origin = origins
        .iter()
        .find(|(k, _)| k == meta.key)
        .map_or(Origin::Default, |(_, o)| o.clone());
    SettingsEntry {
        key: meta.key,
        value: value.to_string(),
        default: meta.default,
        kind: kind_label(meta.kind),
        options: match meta.kind {
            Kind::Enum(vals) => vals
                .iter()
                .map(|v| SettingsEnumOption {
                    name: v.name,
                    desc: v.desc,
                })
                .collect(),
            _ => Vec::new(),
        },
        group: meta.group.as_label(),
        danger: meta.danger.as_str(),
        surfaces: SettingsSurfaces {
            cli: meta.surfaces.cli,
            tui: meta.surfaces.tui,
            gui: meta.surfaces.gui,
        },
        apply: meta.apply.as_str(),
        desc: meta.desc,
        origin: SettingsOrigin {
            label: origin.as_label(),
            path: match origin {
                Origin::UserFile(p) | Origin::ProjectFile(p) => Some(p.display().to_string()),
                _ => None,
            },
        },
        warning: None,
        valid: validate_value(meta, value).is_ok(),
        clamp: pakmap::settings::validate::numeric_clamp(meta.key),
    }
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Bool => "bool",
        Kind::U16 => "u16",
        Kind::U32 => "u32",
        Kind::U64 => "u64",
        Kind::Usize => "usize",
        Kind::Enum(_) => "enum",
        Kind::Style => "style",
        Kind::Text => "text",
        Kind::OptText => "opt-text",
        Kind::PairU16 => "pair",
    }
}

// ---------------------------------------------------------------------------
// Write / unset
// ---------------------------------------------------------------------------

/// `target = "user"` writes the user's XDG config (created from the example
/// template when `create` is true); `target = "project"` writes the
/// discovered project file, never creating one (plan D2). All policy,
/// conflict-gate and atomicity behavior lives in the engine.
#[tauri::command]
pub(crate) fn set_config_entry(
    key: String,
    value: String,
    target: String,
    create: bool,
) -> Result<(), GuiError> {
    let tg: Target = target_to_target(&target, create).map_err(|e| set_err_to_gui(&e))?;
    write::set(&key, &value, &tg)
        .map(|_| ())
        .map_err(|e| set_err_to_gui(&e))
}

/// Remove the key from the target file (missing file / missing key are
/// idempotent no-ops in the engine).
#[tauri::command]
pub(crate) fn unset_config_entry(key: String, target: String) -> Result<(), GuiError> {
    let tg: Target = target_to_target(&target, false).map_err(|e| set_err_to_gui(&e))?;
    write::unset(&key, &tg)
        .map(|_| ())
        .map_err(|e| set_err_to_gui(&e))
}

fn target_to_target(target: &str, create: bool) -> Result<Target, SetError> {
    match target {
        "user" => {
            let path = Config::default_path().ok_or_else(|| {
                SetError::PolicyRefused(
                    "no config directory (XDG_CONFIG_HOME/ HOME?) — cannot write a user config"
                        .to_string(),
                )
            })?;
            // `create` bootstraps a missing user file from the documented
            // example template — an explicit user action, never implicit.
            Ok(Target::User { path, create })
        }
        "project" => {
            let path = Config::project_path().ok_or_else(|| {
                SetError::PolicyRefused(
                    "no project config here (./pakmap.toml or ./pakmap/config.toml) — \
                     nothing to write"
                        .to_string(),
                )
            })?;
            let trusted = std::env::var_os("PAKMAP_TRUST_PROJECT").is_some_and(|v| v == "1");
            Ok(Target::Project { path, trusted })
        }
        other => Err(SetError::PolicyRefused(format!(
            "unknown target '{other}' — expected 'user' or 'project'",
        ))),
    }
}

/// Engine refusal → the documented `{code, message, hint}` envelope. Hints are
/// recovery copy, mirroring the CLI's stderr guidance.
fn set_err_to_gui(e: &SetError) -> GuiError {
    let code = e.code();
    let message = e.message();
    let hint = match code {
        "conflict" => Some(
            "the config changed on disk while editing (external editor or another pakmap?) — \
             reload the settings screen and retry"
                .to_string(),
        ),
        "policy-refused" => Some(
            "project-local [behavior] writes need PAKMAP_TRUST_PROJECT=1; \
             the user config needs the target file to exist first (create from settings)"
                .to_string(),
        ),
        "unparseable" => Some(
            "edit the config file by hand — pakmap refuses to rewrite ambiguous lines".to_string(),
        ),
        "unknown-key" => Some("run `pakmap config docs` for every valid key".to_string()),
        "invalid-value" => Some("the message names the expected shape".to_string()),
        _ => None,
    };
    GuiError {
        code: code.to_string(),
        message,
        hint,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pakmap::settings::catalog::Kind as EngineKind;

    fn expected_kind_label(k: EngineKind) -> &'static str {
        match k {
            EngineKind::Bool => "bool",
            EngineKind::U16 => "u16",
            EngineKind::U32 => "u32",
            EngineKind::U64 => "u64",
            EngineKind::Usize => "usize",
            EngineKind::Enum(_) => "enum",
            EngineKind::Style => "style",
            EngineKind::Text => "text",
            EngineKind::OptText => "opt-text",
            EngineKind::PairU16 => "pair",
        }
    }

    #[test]
    fn kind_label_covers_every_kind() {
        for k in [
            EngineKind::Bool,
            EngineKind::U16,
            EngineKind::U32,
            EngineKind::U64,
            EngineKind::Usize,
            EngineKind::Enum(&[]),
            EngineKind::Style,
            EngineKind::Text,
            EngineKind::OptText,
            EngineKind::PairU16,
        ] {
            assert_eq!(kind_label(k), expected_kind_label(k), "label for {k:?}");
        }
    }

    /// Every catalog row renders with a stable group, danger and apply token —
    /// the row model the frontend relies on cannot silently gain new states.
    /// Origins stay empty (a defaults-only render): the test never touches the
    /// developer's real config files.
    #[test]
    fn catalog_rows_render_fully() {
        let cfg = Config::default();
        let origins: Vec<(String, Origin)> = Vec::new();
        for meta in CATALOG {
            let v = meta.field.get(&cfg);
            let e = render_entry(meta, &origins, &v);
            assert!(!e.group.is_empty(), "group label for {}", meta.key);
            assert!(
                ["", "caution", "danger"].contains(&e.danger),
                "danger token for {}",
                meta.key
            );
            assert!(
                ["immediate", "next-launch", "live-preview"].contains(&e.apply),
                "apply token for {}",
                meta.key
            );
            assert_eq!(e.origin.label, "default", "origin for {}", meta.key);
        }
    }

    /// The GUI exposes writes ONLY for keys whose surfaces include `gui`; the
    /// surface flag must exist for every key (no half-mapped rows).
    #[test]
    fn surfaces_complete() {
        for meta in CATALOG {
            assert!(
                meta.surfaces.cli || meta.surfaces.tui || meta.surfaces.gui,
                "at least one surface for {}",
                meta.key
            );
        }
    }

    /// `target_to_target` refuses unknown targets with the stable error code;
    /// user targets honor the explicit create flag. The project branch is
    /// cwd-dependent (`./pakmap.toml` may or may not exist on the running
    /// machine), so its assertion is conditional — the refusal semantics
    /// themselves are engine territory (write.rs project-policy tests).
    #[test]
    fn unknown_target_is_policy_refused() {
        let err = target_to_target("device", false).expect_err("unknown target refused");
        assert_eq!(err.code(), "policy-refused");
        let user = target_to_target("user", true).expect("user target builds");
        assert!(
            matches!(user, Target::User { create: true, .. }),
            "create flag must reach the engine target"
        );
        if let Err(e) = target_to_target("project", true) {
            assert_eq!(e.code(), "policy-refused");
        } // Some(_) = this machine has a project config; nothing to pin here
    }
}
