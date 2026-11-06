//! pakmap settings engine (plan §1): one shared truth for every writable
//! setting across CLI/TUI/GUI — catalog, layered resolve (D4), lexical
//! scanner, validation and comment-preserving atomic writes (§1.6).

pub mod catalog;
pub mod command;
pub mod origin;
pub mod scanner;
pub mod validate;
pub mod write;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::Config;
use catalog::{CATALOG, SettingMeta};
use origin::Origin;
use scanner::Scan;

/// Deprecated spelling of `search.bottom_up` (plan §1.2): accepted where it
/// has always been, resolved onto the canonical key, warned about.
pub const BOTTOM_UP_ALIAS: &str = "behavior.bottom_up";

/// D2 policy: the only sections a project file may still be written for.
pub const PROJECT_ALLOWED_SECTIONS: &[&str] = &["search", "tui", "theme"];

/// Exhaustive enum over every `Config` field (plan §1.2): `get`/`set` are a
/// single match below, so adding a Config field without updating the catalog
/// fails both the parachute match and the parity test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldId {
    SearchLimit,
    SearchSource,
    SearchBy,
    SearchRegex,
    SearchInstalledOnly,
    SearchTimeoutSecs,
    SearchBottomUp,
    TuiEnabled,
    TuiFloorWidth,
    TuiFloorHeight,
    TuiLayoutSearch,
    TuiLayoutResultsMin,
    TuiBorder,
    TuiHighlightSymbol,
    TuiShowTabs,
    TuiPreviewPane,
    TuiPopupInfo,
    TuiPopupConfirm,
    TuiPopupMessage,
    TuiPopupHelp,
    TuiTickChars,
    TuiSpinnerTemplate,
    TuiDebounceMs,
    TuiPollMs,
    ThemeNoColor,
    ThemeBorderFocused,
    ThemeBorderUnfocused,
    ThemeBorderError,
    ThemeText,
    ThemeTextDim,
    ThemeRepoCore,
    ThemeRepoExtra,
    ThemeRepoMultilib,
    ThemeRepoAur,
    ThemeRepoOther,
    ThemeRepoLocal,
    ThemeInstalled,
    ThemeOutOfDate,
    ThemeVotes,
    ThemeVersion,
    ThemeHighlightBg,
    ThemeHighlightFg,
    ThemeStatusLoading,
    ThemeStatusIdle,
    ThemeHelp,
    ThemePopupTitle,
    ThemePopupBg,
    ThemeTabSelected,
    ThemeTabNormal,
    BehaviorVerbose,
    BehaviorAurRpc,
    BehaviorCacheDir,
    BehaviorMakepkgNoconfirm,
    BehaviorRemoveFlags,
    BehaviorRemoveNoconfirm,
    BehaviorRefreshNoconfirm,
    BehaviorReadonly,
    BehaviorNewsEnabled,
    BehaviorNewsCacheSecs,
    BehaviorDiffPager,
    BehaviorUpgradeNoconfirm,
    BehaviorCleanKeep,
    BehaviorCleanNoconfirm,
    BehaviorFlatpakEnabled,
    BehaviorFlatpakRemote,
    BehaviorFlatpakNoconfirm,
    GuiTheme,
}

impl FieldId {
    /// Display string of the field's current value (raw file spelling per
    /// kind; `None` options render as "" and `gui.theme` normalizes to
    /// "device").
    pub fn get(&self, c: &Config) -> String {
        match *self {
            FieldId::SearchLimit => c.search.limit.to_string(),
            FieldId::SearchSource => c.search.source.clone(),
            FieldId::SearchBy => c.search.by.clone(),
            FieldId::SearchRegex => c.search.regex.to_string(),
            FieldId::SearchInstalledOnly => c.search.installed_only.to_string(),
            FieldId::SearchTimeoutSecs => c.search.timeout_secs.to_string(),
            FieldId::SearchBottomUp => c.search.bottom_up.to_string(),
            FieldId::TuiEnabled => c.tui.enabled.to_string(),
            FieldId::TuiFloorWidth => c.tui.floor_width.to_string(),
            FieldId::TuiFloorHeight => c.tui.floor_height.to_string(),
            FieldId::TuiLayoutSearch => c.tui.layout_search.to_string(),
            FieldId::TuiLayoutResultsMin => c.tui.layout_results_min.to_string(),
            FieldId::TuiBorder => c.tui.border.clone(),
            FieldId::TuiHighlightSymbol => opt_get(&c.tui.highlight_symbol),
            FieldId::TuiShowTabs => c.tui.show_tabs.to_string(),
            FieldId::TuiPreviewPane => c.tui.preview_pane.to_string(),
            FieldId::TuiPopupInfo => pair_get(c.tui.popup_info),
            FieldId::TuiPopupConfirm => pair_get(c.tui.popup_confirm),
            FieldId::TuiPopupMessage => pair_get(c.tui.popup_message),
            FieldId::TuiPopupHelp => pair_get(c.tui.popup_help),
            FieldId::TuiTickChars => opt_get(&c.tui.tick_chars),
            FieldId::TuiSpinnerTemplate => opt_get(&c.tui.spinner_template),
            FieldId::TuiDebounceMs => c.tui.debounce_ms.to_string(),
            FieldId::TuiPollMs => c.tui.poll_ms.to_string(),
            FieldId::ThemeNoColor => c.theme.no_color.to_string(),
            FieldId::ThemeBorderFocused => c.theme.border_focused.clone(),
            FieldId::ThemeBorderUnfocused => c.theme.border_unfocused.clone(),
            FieldId::ThemeBorderError => c.theme.border_error.clone(),
            FieldId::ThemeText => c.theme.text.clone(),
            FieldId::ThemeTextDim => c.theme.text_dim.clone(),
            FieldId::ThemeRepoCore => c.theme.repo_core.clone(),
            FieldId::ThemeRepoExtra => c.theme.repo_extra.clone(),
            FieldId::ThemeRepoMultilib => c.theme.repo_multilib.clone(),
            FieldId::ThemeRepoAur => c.theme.repo_aur.clone(),
            FieldId::ThemeRepoOther => c.theme.repo_other.clone(),
            FieldId::ThemeRepoLocal => c.theme.repo_local.clone(),
            FieldId::ThemeInstalled => c.theme.installed.clone(),
            FieldId::ThemeOutOfDate => c.theme.out_of_date.clone(),
            FieldId::ThemeVotes => c.theme.votes.clone(),
            FieldId::ThemeVersion => c.theme.version.clone(),
            FieldId::ThemeHighlightBg => c.theme.highlight_bg.clone(),
            FieldId::ThemeHighlightFg => c.theme.highlight_fg.clone(),
            FieldId::ThemeStatusLoading => c.theme.status_loading.clone(),
            FieldId::ThemeStatusIdle => c.theme.status_idle.clone(),
            FieldId::ThemeHelp => c.theme.help.clone(),
            FieldId::ThemePopupTitle => c.theme.popup_title.clone(),
            FieldId::ThemePopupBg => c.theme.popup_bg.clone(),
            FieldId::ThemeTabSelected => c.theme.tab_selected.clone(),
            FieldId::ThemeTabNormal => c.theme.tab_normal.clone(),
            FieldId::BehaviorVerbose => c.behavior.verbose.to_string(),
            FieldId::BehaviorAurRpc => opt_get(&c.behavior.aur_rpc),
            FieldId::BehaviorCacheDir => opt_get(&c.behavior.cache_dir),
            FieldId::BehaviorMakepkgNoconfirm => c.behavior.makepkg_noconfirm.to_string(),
            FieldId::BehaviorRemoveFlags => c.behavior.remove_flags.clone(),
            FieldId::BehaviorRemoveNoconfirm => c.behavior.remove_noconfirm.to_string(),
            FieldId::BehaviorRefreshNoconfirm => c.behavior.refresh_noconfirm.to_string(),
            FieldId::BehaviorReadonly => c.behavior.readonly.to_string(),
            FieldId::BehaviorNewsEnabled => c.behavior.news_enabled.to_string(),
            FieldId::BehaviorNewsCacheSecs => c.behavior.news_cache_secs.to_string(),
            FieldId::BehaviorDiffPager => opt_get(&c.behavior.diff_pager),
            FieldId::BehaviorUpgradeNoconfirm => c.behavior.upgrade_noconfirm.to_string(),
            FieldId::BehaviorCleanKeep => c.behavior.clean_keep.to_string(),
            FieldId::BehaviorCleanNoconfirm => c.behavior.clean_noconfirm.to_string(),
            FieldId::BehaviorFlatpakEnabled => c.behavior.flatpak_enabled.to_string(),
            FieldId::BehaviorFlatpakRemote => opt_get(&c.behavior.flatpak_remote),
            FieldId::BehaviorFlatpakNoconfirm => c.behavior.flatpak_noconfirm.to_string(),
            FieldId::GuiTheme => match &c.gui.theme {
                Some(t) if !t.trim().is_empty() => t.clone(),
                _ => "device".to_string(),
            },
        }
    }

    /// Parse `raw` per the field's `Kind` and assign. The engine runs
    /// `validate::validate` first; this parse is the single exact writer.
    pub fn set(&self, c: &mut Config, raw: &str) -> Result<(), String> {
        let t = raw.trim();
        match *self {
            FieldId::SearchLimit => {
                c.search.limit = parse_usize(t)?;
            }
            FieldId::SearchSource => {
                c.search.source = enum_set(catalog::SEARCH_SOURCE_VALUES, t)?;
            }
            FieldId::SearchBy => {
                c.search.by = enum_set(catalog::AUR_BY_VALUES, t)?;
            }
            FieldId::SearchRegex => c.search.regex = parse_bool(t)?,
            FieldId::SearchInstalledOnly => c.search.installed_only = parse_bool(t)?,
            FieldId::SearchTimeoutSecs => c.search.timeout_secs = parse_u64(t)?,
            FieldId::SearchBottomUp => c.search.bottom_up = parse_bool(t)?,
            FieldId::TuiEnabled => c.tui.enabled = parse_bool(t)?,
            FieldId::TuiFloorWidth => c.tui.floor_width = parse_u16(t)?,
            FieldId::TuiFloorHeight => c.tui.floor_height = parse_u16(t)?,
            FieldId::TuiLayoutSearch => c.tui.layout_search = parse_u16(t)?,
            FieldId::TuiLayoutResultsMin => c.tui.layout_results_min = parse_u16(t)?,
            FieldId::TuiBorder => {
                c.tui.border = enum_set(catalog::BORDER_VALUES, t)?;
            }
            FieldId::TuiHighlightSymbol => c.tui.highlight_symbol = opt_text(t),
            FieldId::TuiShowTabs => c.tui.show_tabs = parse_bool(t)?,
            FieldId::TuiPreviewPane => c.tui.preview_pane = parse_bool(t)?,
            FieldId::TuiPopupInfo => c.tui.popup_info = Some(pair_parse(t)?),
            FieldId::TuiPopupConfirm => c.tui.popup_confirm = Some(pair_parse(t)?),
            FieldId::TuiPopupMessage => c.tui.popup_message = Some(pair_parse(t)?),
            FieldId::TuiPopupHelp => c.tui.popup_help = Some(pair_parse(t)?),
            FieldId::TuiTickChars => c.tui.tick_chars = opt_text(t),
            FieldId::TuiSpinnerTemplate => c.tui.spinner_template = opt_text(t),
            FieldId::TuiDebounceMs => c.tui.debounce_ms = parse_u64(t)?,
            FieldId::TuiPollMs => c.tui.poll_ms = parse_u64(t)?,
            FieldId::ThemeNoColor => c.theme.no_color = parse_bool(t)?,
            FieldId::ThemeBorderFocused => c.theme.border_focused = t.to_string(),
            FieldId::ThemeBorderUnfocused => c.theme.border_unfocused = t.to_string(),
            FieldId::ThemeBorderError => c.theme.border_error = t.to_string(),
            FieldId::ThemeText => c.theme.text = t.to_string(),
            FieldId::ThemeTextDim => c.theme.text_dim = t.to_string(),
            FieldId::ThemeRepoCore => c.theme.repo_core = t.to_string(),
            FieldId::ThemeRepoExtra => c.theme.repo_extra = t.to_string(),
            FieldId::ThemeRepoMultilib => c.theme.repo_multilib = t.to_string(),
            FieldId::ThemeRepoAur => c.theme.repo_aur = t.to_string(),
            FieldId::ThemeRepoOther => c.theme.repo_other = t.to_string(),
            FieldId::ThemeRepoLocal => c.theme.repo_local = t.to_string(),
            FieldId::ThemeInstalled => c.theme.installed = t.to_string(),
            FieldId::ThemeOutOfDate => c.theme.out_of_date = t.to_string(),
            FieldId::ThemeVotes => c.theme.votes = t.to_string(),
            FieldId::ThemeVersion => c.theme.version = t.to_string(),
            FieldId::ThemeHighlightBg => c.theme.highlight_bg = t.to_string(),
            FieldId::ThemeHighlightFg => c.theme.highlight_fg = t.to_string(),
            FieldId::ThemeStatusLoading => c.theme.status_loading = t.to_string(),
            FieldId::ThemeStatusIdle => c.theme.status_idle = t.to_string(),
            FieldId::ThemeHelp => c.theme.help = t.to_string(),
            FieldId::ThemePopupTitle => c.theme.popup_title = t.to_string(),
            FieldId::ThemePopupBg => c.theme.popup_bg = t.to_string(),
            FieldId::ThemeTabSelected => c.theme.tab_selected = t.to_string(),
            FieldId::ThemeTabNormal => c.theme.tab_normal = t.to_string(),
            FieldId::BehaviorVerbose => {
                let v = parse_u64(t)?;
                if v > u8::MAX as u64 {
                    return Err("expected 0..=255".into());
                }
                c.behavior.verbose = v as u8;
            }
            FieldId::BehaviorAurRpc => c.behavior.aur_rpc = opt_text(t),
            FieldId::BehaviorCacheDir => c.behavior.cache_dir = opt_text(t),
            FieldId::BehaviorMakepkgNoconfirm => c.behavior.makepkg_noconfirm = parse_bool(t)?,
            FieldId::BehaviorRemoveFlags => c.behavior.remove_flags = t.to_string(),
            FieldId::BehaviorRemoveNoconfirm => c.behavior.remove_noconfirm = parse_bool(t)?,
            FieldId::BehaviorRefreshNoconfirm => c.behavior.refresh_noconfirm = parse_bool(t)?,
            FieldId::BehaviorReadonly => c.behavior.readonly = parse_bool(t)?,
            FieldId::BehaviorNewsEnabled => c.behavior.news_enabled = parse_bool(t)?,
            FieldId::BehaviorNewsCacheSecs => c.behavior.news_cache_secs = parse_u64(t)?,
            FieldId::BehaviorDiffPager => c.behavior.diff_pager = opt_text(t),
            FieldId::BehaviorUpgradeNoconfirm => c.behavior.upgrade_noconfirm = parse_bool(t)?,
            FieldId::BehaviorCleanKeep => {
                let v = usize::try_from(parse_u64(t)?)
                    .map_err(|_| "expected a non-negative number".to_string())?;
                c.behavior.clean_keep =
                    u32::try_from(v).map_err(|_| "value too large for this setting".to_string())?;
            }
            FieldId::BehaviorCleanNoconfirm => c.behavior.clean_noconfirm = parse_bool(t)?,
            FieldId::BehaviorFlatpakEnabled => c.behavior.flatpak_enabled = parse_bool(t)?,
            FieldId::BehaviorFlatpakRemote => c.behavior.flatpak_remote = opt_text(t),
            FieldId::BehaviorFlatpakNoconfirm => c.behavior.flatpak_noconfirm = parse_bool(t)?,
            FieldId::GuiTheme => {
                c.gui.theme = Some(enum_set(catalog::THEME_VALUES, t)?);
            }
        }
        Ok(())
    }
}

fn opt_get(v: &Option<String>) -> String {
    v.clone().unwrap_or_default()
}

fn pair_get(v: Option<[u16; 2]>) -> String {
    v.map(|p| format!("{},{}", p[0], p[1])).unwrap_or_default()
}

fn parse_bool(t: &str) -> Result<bool, String> {
    match t {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err("must be true or false".into()),
    }
}

fn parse_u64(t: &str) -> Result<u64, String> {
    t.parse::<u64>()
        .map_err(|_| "expected a non-negative number".to_string())
}

fn parse_u16(t: &str) -> Result<u16, String> {
    t.parse::<u16>()
        .map_err(|_| "expected a non-negative number".to_string())
}

fn parse_usize(t: &str) -> Result<usize, String> {
    t.parse::<usize>()
        .map_err(|_| "expected a non-negative number".to_string())
}

fn opt_text(t: &str) -> Option<String> {
    if t.trim().is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

fn enum_set(vals: &[catalog::EnumVal], t: &str) -> Result<String, String> {
    if vals.iter().any(|v| v.name == t) {
        Ok(t.to_string())
    } else {
        Err(format!(
            "expected one of [{}]",
            vals.iter().map(|v| v.name).collect::<Vec<_>>().join("|")
        ))
    }
}

fn pair_parse(t: &str) -> Result<[u16; 2], String> {
    let s = t.trim();
    let s = s.strip_prefix('[').unwrap_or(s);
    let s = s.strip_suffix(']').unwrap_or(s);
    let parts: Vec<&str> = s.split(',').map(str::trim).collect();
    if parts.len() != 2 {
        return Err("expected two comma-separated numbers (cols,rows)".into());
    }
    let a: u16 = parts[0]
        .parse()
        .map_err(|_| "expected two comma-separated u16 values".to_string())?;
    let b: u16 = parts[1]
        .parse()
        .map_err(|_| "expected two comma-separated u16 values".to_string())?;
    Ok([a, b])
}

/// Result of `resolve()`: effective config + full-origin map (plan §1.3).
#[derive(Debug, Clone)]
pub struct Resolved {
    pub config: Config,
    /// canonical key → origin, for keys explicitly set in a layer.
    pub origins: Vec<(String, Origin)>,
    /// Deprecated aliases seen and resolved (`behavior.bottom_up`).
    pub aliases: Vec<String>,
    /// unknown keys, invalid values, policy notices (fail-open, D6).
    pub warnings: Vec<String>,
    pub user_path: Option<PathBuf>,
    pub project_path: Option<PathBuf>,
    pub project_trusted: bool,
}

/// Resolve the effective config for this process (plan §1.3): layer order
/// defaults < user file < project file (allowed sections, D2) — env/CLI stay
/// annotated at the op layer and are not merged here.
pub fn resolve(cli_cfg: Option<&Path>, env: &dyn Fn(&str) -> Option<String>) -> Resolved {
    let trusted = env("PAKMAP_TRUST_PROJECT").as_deref() == Some("1");
    // `--config PATH` is a trusted user-layer REPLACEMENT (plan §1.3): the
    // XDG file is skipped entirely when it is given.
    let user = cli_cfg.map(Path::to_path_buf).or_else(Config::default_path);
    let project = Config::project_path();
    resolve_layers(user.as_deref(), project.as_deref(), trusted)
}

/// Path-injection variant for unit tests; `resolve()` drives it with real
/// discovery. `user`/`project` overwrite candidate paths.
pub(crate) fn resolve_layers(
    user: Option<&Path>,
    project: Option<&Path>,
    trusted: bool,
) -> Resolved {
    let mut cfg = Config::default();
    let mut state = MergeState::default();

    let mut user_path = user.map(Path::to_path_buf);
    let mut project_path = project.map(Path::to_path_buf);

    if let Some(p) = user {
        match std::fs::read_to_string(p) {
            Err(e) => {
                if p.exists() {
                    state
                        .warnings
                        .push(format!("warn: cannot read {}: {e}", p.display()));
                }
                user_path = Some(p.to_path_buf());
            }
            Ok(text) => {
                user_path = Some(p.to_path_buf());
                // Trusted user layer: full parse (--config PATH has always
                // been trusted). Unparseable → warn + skip layer (fail-open).
                match toml::from_str::<Config>(&text).map(Config::normalized) {
                    Ok(layer) => apply_layer(
                        &mut cfg,
                        &layer,
                        &text,
                        p,
                        Origin::UserFile(p.to_path_buf()),
                        true,
                        &mut state,
                    ),
                    Err(e) => {
                        state
                            .warnings
                            .push(format!("warn: invalid config {}: {}", p.display(), e))
                    }
                }
            }
        }
    }

    if let Some(p) = project {
        match std::fs::read_to_string(p) {
            Err(e) => {
                if p.exists() {
                    state
                        .warnings
                        .push(format!("warn: cannot read {}: {e}", p.display()));
                }
                project_path = None;
            }
            Ok(text) => {
                project_path = Some(p.to_path_buf());
                // Project policy (D2/D4): `[search]/[tui]/[theme]` merge in
                // per explicit key; the whole `[behavior]` family is stripped
                // lexically unless PAKMAP_TRUST_PROJECT=1 — kept byte-exact
                // with `load_project_checked`'s refusal text. The scanner runs
                // on the ORIGINAL text; values resolve from serde's parse of
                // the (possibly stripped) effective text.
                let (effective_text, had_behavior) = if trusted {
                    (text.clone(), false)
                } else {
                    Config::strip_behavior_section(&text)
                };
                if had_behavior && !trusted {
                    state.warnings.push(format!(
                        "note: ignoring [behavior] from {} (project-local config is untrusted; \
                         set PAKMAP_TRUST_PROJECT=1 to trust this directory's file)",
                        p.display()
                    ));
                }
                match toml::from_str::<Config>(&effective_text).map(Config::normalized) {
                    Ok(layer) => {
                        apply_layer(
                            &mut cfg,
                            &layer,
                            &text,
                            p,
                            Origin::ProjectFile(p.to_path_buf()),
                            trusted,
                            &mut state,
                        );
                    }
                    Err(e) => {
                        state
                            .warnings
                            .push(format!("warn: invalid config {}: {}", p.display(), e))
                    }
                }
            }
        }
    }

    // Origins in catalog order for stable `config list` output.
    let mut ordered = Vec::with_capacity(state.origins.len());
    for meta in CATALOG {
        if let Some(o) = state.origins.remove(meta.key) {
            ordered.push((meta.key.to_string(), o));
        }
    }
    ordered.extend(state.origins);

    Resolved {
        config: cfg,
        origins: ordered,
        aliases: state.aliases,
        warnings: state.warnings,
        user_path,
        project_path,
        project_trusted: trusted,
    }
}

/// Layer-merge accumulator threaded through the passes below as one mutable
/// handle, so neither pass needs per-accumulator parameters (clippy
/// `too_many_arguments`). Drained by `resolve_layers` at the end.
#[derive(Default)]
struct MergeState {
    origins: HashMap<String, Origin>,
    aliases: Vec<String>,
    warnings: Vec<String>,
}

/// Apply ONE file layer onto `cfg`, per D4's explicit-keys-only merge: only
/// keys the lexical scanner found in the file are copied over (values come
/// from serde's full parse of the same file, so line shape can never corrupt
/// a value); unknown keys and the deprecated alias emit warnings (D6).
fn apply_layer(
    cfg: &mut Config,
    layer: &Config,
    text: &str,
    path: &Path,
    origin: Origin,
    allow_behavior: bool,
    state: &mut MergeState,
) {
    let scan = scanner::scan(text);
    for found in &scan.keys {
        if found.key == BOTTOM_UP_ALIAS {
            continue; // handled in the alias pass below (two-pass policy)
        }
        match SettingMeta::lookup(&found.key) {
            Some(meta) => {
                let val = meta.field.get(layer);
                if meta.field.set(cfg, &val).is_ok() {
                    state.origins.insert(found.key.clone(), origin.clone());
                } else {
                    // serde accepts it (plain String field) but the kind is
                    // stricter; keep the default and tell the user (D6).
                    state.warnings.push(format!(
                        "warn: invalid value for '{key}' in {path}:{line} — value ignored",
                        key = found.key,
                        path = path.display(),
                        line = found.line_no
                    ));
                }
            }
            None => state.warnings.push(format!(
                "warn: unknown key '{key}' in {path}:{line} — not a pakmap setting",
                key = found.key,
                path = path.display(),
                line = found.line_no
            )),
        }
    }
    apply_alias_layer(&scan, layer, path, origin, allow_behavior, cfg, state);
}

/// `[behavior].bottom_up` alias policy (plan §1.2): when `[search].bottom_up`
/// is explicit in the SAME file it wins; otherwise the alias resolves onto
/// `search.bottom_up` (value from serde's parse) with a migration warning.
/// In an untrusted project file the family strip already ignored it.
fn apply_alias_layer(
    scan: &Scan,
    layer: &Config,
    path: &Path,
    origin: Origin,
    allow_behavior: bool,
    cfg: &mut Config,
    state: &mut MergeState,
) {
    let hits: Vec<&scanner::Found> = scan
        .keys
        .iter()
        .filter(|f| f.key == BOTTOM_UP_ALIAS)
        .collect();
    if hits.is_empty() {
        return;
    }
    let search_explicit = scan.keys.iter().any(|f| f.key == "search.bottom_up");
    if search_explicit {
        for h in hits {
            state.warnings.push(format!(
                "note: ignoring deprecated [behavior] bottom_up in {path}:{line} — \
                 [search] bottom_up takes precedence",
                path = path.display(),
                line = h.line_no
            ));
        }
        return;
    }
    if !allow_behavior {
        return; // untrusted project file: [behavior] is already refused
    }
    if !state.aliases.iter().any(|a| a == BOTTOM_UP_ALIAS) {
        state.aliases.push(BOTTOM_UP_ALIAS.to_string());
    }
    // bool→bool is exact: no parse, no failure path — alias always resolves.
    cfg.search.bottom_up = layer.behavior.bottom_up;
    state.origins.insert("search.bottom_up".to_string(), origin);
    for h in hits {
        state.warnings.push(format!(
            "note: [behavior] bottom_up is a deprecated alias of [search] bottom_up \
             ({path}:{line}) — update your config",
            path = path.display(),
            line = h.line_no
        ));
    }
}

#[cfg(test)]
mod tests {
    //! Parity gates of plan §1.4 (hard tests): catalog ↔ Config ↔ kinds ↔
    //! layered resolve ↔ aliases ↔ fail-open policy. Unwraps are re-allowed
    //! for the test crate via `lib.rs`'s cfg_attr.

    use super::*;

    fn origin_of(r: &Resolved, key: &str) -> Option<Origin> {
        r.origins
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, o)| o.clone())
    }

    /// A probe config with every `Option` filled: serde skips `None`, so
    /// serialized keys only cover the catalog surface when all are set.
    fn probe_all_options() -> Config {
        let mut c = Config::default();
        c.tui.highlight_symbol = Some("x".into());
        c.tui.tick_chars = Some("x".into());
        c.tui.spinner_template = Some("x".into());
        c.tui.popup_info = Some([1, 2]);
        c.tui.popup_confirm = Some([1, 2]);
        c.tui.popup_message = Some([1, 2]);
        c.tui.popup_help = Some([1, 2]);
        c.behavior.aur_rpc = Some("https://aur.archlinux.org/rpc/v5".into());
        c.behavior.cache_dir = Some("/tmp/x".into());
        c.behavior.diff_pager = Some("cat".into());
        c.behavior.flatpak_remote = Some("flathub".into());
        c.gui.theme = Some("device".into());
        c
    }

    /// §1.4.1: serialized `Config` keys cover CATALOG one-to-one. The ONLY
    /// sanctioned extra is `behavior.bottom_up` (deprecated alias kept as a
    /// real field for the resolver; catalog never lists it — plan §1.2).
    #[test]
    fn catalog_parity() {
        let text = toml::to_string(&probe_all_options()).unwrap();
        let value: toml::Value = toml::from_str(&text).unwrap();
        let mut got = Vec::<String>::new();
        if let toml::Value::Table(sections) = &value {
            for (section, inner) in sections {
                if let toml::Value::Table(fields) = inner {
                    for field in fields.keys() {
                        got.push(format!("{section}.{field}"));
                    }
                }
            }
        }
        let mut expected: Vec<String> = CATALOG.iter().map(|m| m.key.to_string()).collect();
        got.sort();
        expected.sort();
        let extra: Vec<String> = got
            .iter()
            .filter(|k| !expected.contains(k))
            .cloned()
            .collect();
        assert_eq!(extra, vec!["behavior.bottom_up".to_string()]);
        got.retain(|k| k != "behavior.bottom_up");
        assert_eq!(got, expected);
    }

    /// §1.4.2: every catalog default parses back and equals `get(default)`.
    #[test]
    fn default_render_parity() {
        for meta in CATALOG {
            let mut probe = Config::default();
            meta.field
                .set(&mut probe, meta.default)
                .expect("catalog default must be set()-parsable");
            assert_eq!(meta.field.get(&probe), meta.default, "{}", meta.key);
        }
    }

    /// §1.4.3: per-kind representative set(sample) → get == sample.
    #[test]
    fn set_get_roundtrip() {
        for meta in CATALOG {
            let sample: String = match meta.kind {
                catalog::Kind::Bool => "true".to_string(),
                catalog::Kind::U16 => "16".to_string(),
                catalog::Kind::U32 => "7".to_string(),
                catalog::Kind::U64 => "900".to_string(),
                catalog::Kind::Usize => "12".to_string(),
                catalog::Kind::Enum(vals) => vals
                    .iter()
                    .map(|v| v.name)
                    .find(|n| *n != meta.default)
                    .unwrap_or(meta.default)
                    .to_string(),
                catalog::Kind::Style => "red bold".to_string(),
                catalog::Kind::Text => "value".to_string(),
                catalog::Kind::OptText => "sample".to_string(),
                catalog::Kind::PairU16 => "66,22".to_string(),
            };
            let mut probe = Config::default();
            meta.field
                .set(&mut probe, &sample)
                .unwrap_or_else(|e| panic!("{}: {e}", meta.key));
            assert_eq!(meta.field.get(&probe), sample, "{}", meta.key);
        }
    }

    /// §1.4.4 with §1.3's normative order (defaults < user < project): the
    /// last layer that EXPLICITLY sets a key wins; a layer that does not set
    /// the key never shadows an earlier layer; a `[behavior]`-only project
    /// file never shadows user keys.
    #[test]
    fn origin_priority_explicit_last_layer_wins() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("user.toml");
        let proj = dir.path().join("pakmap.toml");
        std::fs::write(&user, "[search]\nlimit = 100\n[theme]\ntext = \"white\"\n").unwrap();
        std::fs::write(&proj, "[search]\nlimit = 7\n[theme]\ntext = \"cyan\"\n").unwrap();
        let r = resolve_layers(Some(&user), Some(&proj), false);
        assert_eq!(r.config.search.limit, 7);
        assert_eq!(r.config.theme.text, "cyan");
        assert_eq!(
            origin_of(&r, "search.limit"),
            Some(Origin::ProjectFile(proj.clone()))
        );
        assert_eq!(
            origin_of(&r, "theme.text"),
            Some(Origin::ProjectFile(proj.clone()))
        );

        // Project file without [theme]: the user layer's theme survives.
        let proj2 = dir.path().join("other.toml");
        std::fs::write(&proj2, "[search]\nlimit = 7\n").unwrap();
        let r = resolve_layers(Some(&user), Some(&proj2), false);
        assert_eq!(r.config.theme.text, "white");
        assert_eq!(
            origin_of(&r, "theme.text"),
            Some(Origin::UserFile(user.clone()))
        );

        // Behavior-only project file (untrusted): user keys unaffected.
        let proj3 = dir.path().join("evil.toml");
        std::fs::write(
            &proj3,
            "[behavior]\nreadonly = true\naur_rpc = \"http://attacker.example\"\n",
        )
        .unwrap();
        let r = resolve_layers(Some(&user), Some(&proj3), false);
        assert_eq!(r.config.search.limit, 100);
        assert!(!r.config.behavior.readonly);
        assert!(r.warnings.iter().any(|w| w.contains("ignoring [behavior]")));
    }

    /// §1.4.5: the deprecated alias resolves onto `search.bottom_up` with a
    /// migration warning; an explicit `[search].bottom_up` in the SAME file
    /// wins and the alias is only flagged; an untrusted project file's
    /// `[behavior]` is refused before the alias pass ever runs.
    #[test]
    fn alias_bottom_up() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("user.toml");
        std::fs::write(&user, "[behavior]\nbottom_up = true\n").unwrap();
        let r = resolve_layers(Some(&user), None, false);
        assert!(r.config.search.bottom_up);
        assert_eq!(r.aliases, vec![BOTTOM_UP_ALIAS.to_string()]);
        assert_eq!(
            origin_of(&r, "search.bottom_up"),
            Some(Origin::UserFile(user.clone()))
        );
        assert!(r.warnings.iter().any(|w| w.contains("deprecated alias")));

        // Both spellings in one file: canonical wins, alias ignored (D4-adjacent).
        let both = dir.path().join("both.toml");
        std::fs::write(
            &both,
            "[behavior]\nbottom_up = true\n[search]\nbottom_up = false\n",
        )
        .unwrap();
        let r = resolve_layers(Some(&both), None, false);
        assert!(!r.config.search.bottom_up);
        assert!(r.aliases.is_empty());
        assert!(r.warnings.iter().any(|w| w.contains("takes precedence")));

        // Alias inside an untrusted project file: only the strip note; the
        // canonical [search] key resolves only from the user layer here.
        let plain = dir.path().join("plain.toml");
        std::fs::write(&plain, "[theme]\ntext = \"white\"\n").unwrap();
        let proj = dir.path().join("pakmap.toml");
        std::fs::write(&proj, "[behavior]\nbottom_up = true\n").unwrap();
        let r = resolve_layers(Some(&plain), Some(&proj), false);
        assert!(!r.config.search.bottom_up, "user layer set no bottom_up");
        assert!(r.aliases.is_empty());
        assert!(r.warnings.iter().any(|w| w.contains("ignoring [behavior]")));
    }

    /// §1.4.6 (D6): unknown keys warn only; the parse still loads the value.
    #[test]
    fn unknown_keys_warn_only() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("user.toml");
        std::fs::write(&user, "[search]\nlimit = 42\ntypo = 5\n").unwrap();
        let r = resolve_layers(Some(&user), None, false);
        assert_eq!(r.config.search.limit, 42);
        assert!(
            r.warnings
                .iter()
                .any(|w| w.contains("unknown key 'search.typo'"))
        );
    }

    /// D6 companion: serde accepts a free-form string where the KIND is
    /// stricter — the value is ignored with a warning, default kept.
    #[test]
    fn strict_kind_mismatch_warns_and_keeps_default() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("user.toml");
        std::fs::write(&user, "[search]\nsource = \"weird\"\n").unwrap();
        let r = resolve_layers(Some(&user), None, false);
        assert_eq!(r.config.search.source, "all");
        assert!(
            r.warnings
                .iter()
                .any(|w| w.contains("invalid value for 'search.source'"))
        );
        assert_eq!(
            origin_of(&r, "search.source"),
            None,
            "no origin for kept default"
        );
    }
}
