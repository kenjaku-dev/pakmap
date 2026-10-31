use std::path::{Path, PathBuf};

use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::BorderType;
use serde::{Deserialize, Serialize};

// ---- helpers ---------------------------------------------------------------

fn default_limit() -> usize {
    50
}
fn default_source() -> String {
    "all".into()
}
fn default_by() -> String {
    "name-desc".into()
}
fn default_timeout() -> u64 {
    15
}
fn default_true() -> bool {
    true
}
fn default_border() -> String {
    "rounded".into()
}
fn default_remove_flags() -> String {
    "Rs".into()
}
fn default_repo_local() -> String {
    "green bold".into()
}
fn default_tab_selected() -> String {
    "black on cyan bold".into()
}
fn default_tab_normal() -> String {
    "dark_gray".into()
}
fn default_news_ttl() -> u64 {
    86400
}
fn default_clean_keep() -> u32 {
    2
}

// ---- top-level -------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub search: SearchConfig,
    pub tui: TuiConfig,
    pub theme: ThemeConfig,
    pub behavior: BehaviorConfig,
}

#[allow(clippy::derivable_impls)]
impl Default for Config {
    fn default() -> Self {
        Self {
            search: SearchConfig::default(),
            tui: TuiConfig::default(),
            theme: ThemeConfig::default(),
            behavior: BehaviorConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchConfig {
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default = "default_by")]
    pub by: String,
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub installed_only: bool,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub bottom_up: bool,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            limit: 50,
            source: "all".into(),
            by: "name-desc".into(),
            regex: false,
            installed_only: false,
            timeout_secs: 15,
            bottom_up: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TuiConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub floor_width: u16,
    pub floor_height: u16,
    pub layout_search: u16,
    pub layout_results_min: u16,
    #[serde(default = "default_border")]
    pub border: String,
    #[serde(default)]
    pub highlight_symbol: Option<String>,
    #[serde(default = "default_true")]
    pub show_tabs: bool,
    pub popup_info: Option<[u16; 2]>,
    pub popup_confirm: Option<[u16; 2]>,
    pub popup_message: Option<[u16; 2]>,
    pub popup_help: Option<[u16; 2]>,
    #[serde(default)]
    pub tick_chars: Option<String>,
    #[serde(default)]
    pub spinner_template: Option<String>,
    pub debounce_ms: u64,
    pub poll_ms: u64,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            floor_width: 60,
            floor_height: 14,
            layout_search: 3,
            layout_results_min: 8,
            border: "rounded".into(),
            highlight_symbol: None,
            show_tabs: true,
            popup_info: Some([70, 60]),
            popup_confirm: Some([60, 30]),
            popup_message: Some([60, 20]),
            popup_help: Some([70, 60]),
            tick_chars: None,
            spinner_template: None,
            debounce_ms: 400,
            poll_ms: 200,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    #[serde(default)]
    pub no_color: bool,
    pub border_focused: String,
    pub border_unfocused: String,
    pub border_error: String,
    pub text: String,
    pub text_dim: String,
    pub repo_core: String,
    pub repo_extra: String,
    pub repo_multilib: String,
    pub repo_aur: String,
    pub repo_other: String,
    #[serde(default = "default_repo_local")]
    pub repo_local: String,
    pub installed: String,
    pub out_of_date: String,
    pub votes: String,
    pub version: String,
    pub highlight_bg: String,
    pub highlight_fg: String,
    pub status_loading: String,
    pub status_idle: String,
    pub help: String,
    pub popup_title: String,
    pub popup_bg: String,
    #[serde(default = "default_tab_selected")]
    pub tab_selected: String,
    #[serde(default = "default_tab_normal")]
    pub tab_normal: String,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            no_color: false,
            border_focused: "cyan".into(),
            border_unfocused: "dark_gray".into(),
            border_error: "red bold".into(),
            text: "white".into(),
            text_dim: "dark_gray".into(),
            repo_core: "red bold".into(),
            repo_extra: "green bold".into(),
            repo_multilib: "blue bold".into(),
            repo_aur: "magenta bold".into(),
            repo_other: "cyan bold".into(),
            repo_local: "green bold".into(),
            installed: "green bold".into(),
            out_of_date: "red bold".into(),
            votes: "yellow".into(),
            version: "white".into(),
            highlight_bg: "dark_gray".into(),
            highlight_fg: "white bold".into(),
            status_loading: "cyan".into(),
            status_idle: "gray".into(),
            help: "dark_gray".into(),
            popup_title: "yellow".into(),
            popup_bg: "black".into(),
            tab_selected: "black on cyan bold".into(),
            tab_normal: "dark_gray".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BehaviorConfig {
    #[serde(default)]
    pub bottom_up: bool,
    #[serde(default)]
    pub verbose: u8,
    #[serde(default)]
    pub aur_rpc: Option<String>,
    #[serde(default)]
    pub cache_dir: Option<String>,
    #[serde(default)]
    pub makepkg_noconfirm: bool,
    #[serde(default = "default_remove_flags")]
    pub remove_flags: String,
    #[serde(default)]
    pub remove_noconfirm: bool,
    #[serde(default)]
    pub refresh_noconfirm: bool,
    /// 0.5.0: refuse all mutating ops (install/remove/refresh/upgrade).
    /// For cautious setups (upstream moson-mo/pacseek#42). CLI --readonly wins.
    #[serde(default)]
    pub readonly: bool,
    /// 0.5.0: Arch-news manual-intervention guard before upgrade/refresh.
    #[serde(default = "default_true")]
    pub news_enabled: bool,
    #[serde(default = "default_news_ttl")]
    pub news_cache_secs: u64,
    /// 0.5.0: pager for PKGBUILD diff preview (bat|less|cat|auto).
    #[serde(default)]
    pub diff_pager: Option<String>,
    /// 0.5.0: pass --noconfirm to system upgrade (sudo pacman -Syu).
    #[serde(default)]
    pub upgrade_noconfirm: bool,
    /// 5.1: keep N package versions when cleaning (paccache -k).
    #[serde(default = "default_clean_keep")]
    pub clean_keep: u32,
    /// 5.1: non-interactive clean (sudo -n fails fast instead of prompting).
    #[serde(default)]
    pub clean_noconfirm: bool,
    /// P1 flatpak seam: enable flatpak backend (default true; fail-open when
    /// the `flatpak` binary is absent).
    #[serde(default = "default_true")]
    pub flatpak_enabled: bool,
    /// P1 flatpak seam: remote used for install/search (default flathub).
    #[serde(default)]
    pub flatpak_remote: Option<String>,
    /// P3: non-interactive flatpak install/remove/clean (`-y`).
    #[serde(default)]
    pub flatpak_noconfirm: bool,
}

#[allow(clippy::derivable_impls)]
impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            bottom_up: false,
            verbose: 0,
            aur_rpc: None,
            cache_dir: None,
            makepkg_noconfirm: false,
            remove_flags: default_remove_flags(),
            remove_noconfirm: false,
            refresh_noconfirm: false,
            readonly: false,
            news_enabled: true,
            news_cache_secs: default_news_ttl(),
            diff_pager: None,
            upgrade_noconfirm: false,
            clean_keep: default_clean_keep(),
            clean_noconfirm: false,
            flatpak_enabled: true,
            flatpak_remote: None,
            flatpak_noconfirm: false,
        }
    }
}

// ---- path & load -----------------------------------------------------------

impl Config {
    /// XDG path: $XDG_CONFIG_HOME/pakmap/config.toml or ~/.config/pakmap/config.toml
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("pakmap").join("config.toml"))
    }

    /// Project-local override: ./pakmap.toml or ./pakmap/config.toml (cwd)
    pub fn project_path() -> Option<PathBuf> {
        let cwd = std::env::current_dir().ok()?;
        [
            cwd.join("pakmap.toml"),
            cwd.join("pakmap").join("config.toml"),
        ]
        .into_iter()
        .find(|cand| cand.exists())
    }

    pub fn load() -> Self {
        // v0.9.3 Phase 3: the project-local file is parsed from the working
        // directory — exactly where a repo owner (or an attacker's clone)
        // controls every byte. It no longer wins wholesale: see
        // `load_project` (behavior keys are ignored by default).
        if let Some(p) = Self::project_path()
            && let Some(c) = Self::load_project(&p)
        {
            return c;
        }
        if let Some(p) = Self::default_path()
            && let Some(c) = Self::load_from(&p)
        {
            return c;
        }
        Self::default()
    }

    pub fn load_from(path: &Path) -> Option<Self> {
        let s = std::fs::read_to_string(path).ok()?;
        Self::parse_trusted(&s, path)
    }

    /// Explicitly trusted parse (XDG config and `--config PATH`). Keep the
    /// old wholesale semantics here; the isolation below never applies to an
    /// explicitly passed path — the user typed it on purpose.
    fn parse_trusted(s: &str, path: &Path) -> Option<Self> {
        match toml::from_str::<Self>(s) {
            Ok(cfg) => Some(cfg.normalized()),
            Err(e) => {
                eprintln!("warn: invalid config {}: {}", path.display(), e);
                None
            }
        }
    }

    fn normalized(mut self) -> Self {
        // normalize: empty strings keep defaults
        if self.search.source.trim().is_empty() {
            self.search.source = SearchConfig::default().source;
        }
        if self.search.by.trim().is_empty() {
            self.search.by = SearchConfig::default().by;
        }
        self
    }

    /// Project-local config entry point: honors the PAKMAP_TRUST_PROJECT=1
    /// opt-in (restores the pre-0.9.3 wholesale precedence).
    pub fn load_project(path: &Path) -> Option<Self> {
        let trusted = std::env::var_os("PAKMAP_TRUST_PROJECT").is_some_and(|v| v == "1");
        Self::load_project_checked(path, trusted)
    }

    /// v0.9.3 Phase 3 — project-local config isolation.
    ///
    /// Threat (Code Scanning, CWE-94/377 class): `./pakmap.toml` is parsed
    /// from the working directory — where a repo owner, a git checkout or a
    /// malware download controls every byte. `[behavior]` keys are the
    /// dangerous half: they point AUR RPC calls (network, JSON) at attacker
    /// URLs, redirect the cache dir, or flip `readonly`/noconfirm flags.
    /// Policy, additive to the wire: a real, plain, user-owned file is still
    /// parsed for `[search]`/`[tui]`/`[theme]`; its entire `[behavior]`
    /// section is ignored with a one-line notice. `PAKMAP_TRUST_PROJECT=1`
    /// restores full trust for setups that curate the directory themselves,
    /// and symlinked/foreign-owned/special files are refused outright (via
    /// `safety::fs`) with the loader falling back to the XDG config.
    pub fn load_project_checked(path: &Path, trusted: bool) -> Option<Self> {
        if let Err(e) = crate::safety::fs::ensure_owned_file(path) {
            eprintln!(
                "warn: ignoring project config {} — {e}; falling back to your XDG config",
                path.display()
            );
            return None;
        }
        let s = std::fs::read_to_string(path).ok()?;
        if trusted {
            return Self::parse_trusted(&s, path);
        }
        // Lexical strip, not a `toml::Value` round-trip: parsing into the
        // generic TOML value tree pulls serde's whole generic-Value visitor
        // machinery into the release link (measured ~150 KB of visitors +
        // unwind tables in the shipped GUI binary for a ~10-line job). TOML
        // sections are line-delimited by spec — a header line starts a
        // section and either EOF or the next header line ends it — so a
        // line filter is exact for section removal. Failure modes stay
        // fail-closed: strip output that no longer parses is reported by
        // `parse_trusted` as an invalid config, falling back to XDG.
        let (stripped, had_behavior) = Self::strip_behavior_section(&s);
        if had_behavior {
            eprintln!(
                "note: ignoring [behavior] from {} (project-local config is untrusted; \
                 set PAKMAP_TRUST_PROJECT=1 to trust this directory's file)",
                path.display()
            );
        }
        Self::parse_trusted(&stripped, path)
    }

    /// True when a TOML line is a top-level header naming the `behavior`
    /// family: `[behavior]`, `[[behavior]]`, or any `[behavior.*]` subtable.
    fn is_behavior_header(line: &str) -> bool {
        let t = line.trim_start();
        let name = if let Some(rest) = t.strip_prefix("[[") {
            rest
        } else if let Some(rest) = t.strip_prefix('[') {
            rest
        } else {
            return false;
        };
        let Some(close) = name.find(']') else {
            return false;
        };
        let key = name[..close].trim();
        // Quoted keys are legal header spellings too (`["behavior"]`), both
        // quote styles resolve the same on the strip path.
        let key = if key.len() >= 2
            && ((key.starts_with('"') && key.ends_with('"'))
                || (key.starts_with('\'') && key.ends_with('\'')))
        {
            &key[1..key.len() - 1]
        } else {
            key
        };
        // Header keys are case-sensitive in TOML; only the exact family
        // (`[behaviors]` or `[behavioral]` are NOT the behavior section).
        key.split('.').next() == Some("behavior")
    }

    /// Remove every TOML section whose header names the `behavior` family.
    /// Returns the filtered text and whether anything was removed. A section
    /// ends at the next header line (any `[` column start after trim) or EOF,
    /// per TOML's table structure; comments and quoted keys elsewhere can
    /// never start a header, and the only conceivable mis-strip is a
    /// multi-line string whose continuation line *is* a header-looking line —
    /// which fails the follow-up parse and falls back to XDG (fail-closed).
    fn strip_behavior_section(source: &str) -> (String, bool) {
        let mut out = String::with_capacity(source.len());
        let mut dropping = false;
        let mut removed = false;
        for line in source.lines() {
            if Self::is_behavior_header(line) {
                dropping = true;
                removed = true;
                continue;
            }
            if dropping {
                if line.trim_start().starts_with('[') {
                    dropping = false; // next section starts: keep this line
                } else {
                    continue; // still inside the dropped section
                }
            }
            out.push_str(line);
            out.push('\n');
        }
        (out, removed)
    }

    /// Write example file to default_path (create dirs). Returns path or error string.
    pub fn init_example() -> Result<PathBuf, String> {
        let path = Self::default_path().ok_or_else(|| "no config dir".to_string())?;
        if path.exists() {
            return Err(format!("already exists: {}", path.display()));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let example = Self::example_toml();
        std::fs::write(&path, example).map_err(|e| e.to_string())?;
        Ok(path)
    }

    pub fn example_toml() -> String {
        r#"# pakmap config — edit and save, changes apply on next launch
# See: https://github.com/kenjaku-dev/pakmap  — all fields optional

[search]
limit = 50                 # 0 = no limit
source = "all"             # aur|repo|flatpak|all
by = "name-desc"           # name|name-desc|maintainer|depends|makedepends|optdepends|checkdepends
regex = false
installed_only = false
timeout_secs = 15
bottom_up = false

[tui]
floor_width = 60
floor_height = 14          # 13 + 1 tabs bar (Search/Installed)
layout_search = 3
layout_results_min = 8
border = "rounded"         # rounded|plain|double|thick
show_tabs = true           # Tab bar on top: Search / Installed
highlight_symbol = "▸ "   # set "" to disable
popup_info = [70, 60]
popup_confirm = [60, 30]
popup_message = [60, 20]
popup_help = [70, 60]
# tick_chars = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ "
# spinner_template = "{spinner:.cyan} {msg}"
debounce_ms = 400
poll_ms = 200

[theme]
no_color = false
border_focused = "cyan"
border_unfocused = "dark_gray"
border_error = "red bold"
text = "white"
text_dim = "dark_gray"
repo_core = "red bold"
repo_extra = "green bold"
repo_multilib = "blue bold"
repo_aur = "magenta bold"
repo_other = "cyan bold"
repo_local = "green bold"
installed = "green bold"
out_of_date = "red bold"
votes = "yellow"
version = "white"
highlight_bg = "dark_gray"
highlight_fg = "white bold"
status_loading = "cyan"
status_idle = "gray"
help = "dark_gray"
popup_title = "yellow"
popup_bg = "black"
tab_selected = "black on cyan bold"
tab_normal = "dark_gray"

[behavior]
bottom_up = false
verbose = 0
# aur_rpc = "https://aur.archlinux.org/rpc/v5"
# cache_dir = "/tmp/pakmap"
# PAKMAP_NOCONFIRM=1 in the environment also enables the two flags below
# (and skips --remove/--refresh confirmations) — handy for scripts.
makepkg_noconfirm = false
remove_flags = "Rs"        # R|Rs|Rns|Ru — sudo pacman -<flags>
remove_noconfirm = false   # add --noconfirm to remove
refresh_noconfirm = false  # add --noconfirm to refresh (pacman -Sy)
readonly = false           # refuse install/remove/refresh/upgrade (safe browse)
news_enabled = true        # Arch-news intervention guard before upgrade
news_cache_secs = 86400    # news feed cache TTL
# diff_pager = "auto"       # auto|bat|less|cat for PKGBUILD diff preview
upgrade_noconfirm = false  # add --noconfirm to system upgrade
clean_keep = 2             # paccache -k: versions to keep on --clean (min 1)
clean_noconfirm = false    # non-interactive clean (sudo -n, fails fast)
flatpak_enabled = true     # P1 seam: flatpak backend (fail-open when binary absent)
# flatpak_remote = "flathub" # P1 seam: remote for install/search
flatpak_noconfirm = false    # P3: -y for flatpak install/remove/clean
"#
        .into()
    }
}

// ---- theme helpers ---------------------------------------------------------

pub fn parse_color(s: &str) -> Color {
    match s.trim().to_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" | "bright_magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" | "grey" => Color::Gray,
        "dark_gray" | "darkgray" | "dark-grey" => Color::DarkGray,
        "white" => Color::White,
        "reset" => Color::Reset,
        _ => Color::Reset,
    }
}

pub fn parse_style(s: &str) -> Style {
    let mut style = Style::default();
    // Support "fg on bg modifiers", e.g. "black on cyan bold", plus legacy "cyan bold".
    // Tokens split on space/comma/plus; "on" consumes the next color token as background.
    let tokens: Vec<String> = s
        .split([' ', ',', '+'])
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i].as_str();
        match t {
            "none" => {}
            "reset" => style = Style::default(),
            "bold" => style = style.add_modifier(Modifier::BOLD),
            "dim" => style = style.add_modifier(Modifier::DIM),
            "italic" => style = style.add_modifier(Modifier::ITALIC),
            "underlined" | "underline" => style = style.add_modifier(Modifier::UNDERLINED),
            "reversed" | "reverse" | "invert" => style = style.add_modifier(Modifier::REVERSED),
            "crossedout" | "crossed_out" => style = style.add_modifier(Modifier::CROSSED_OUT),
            "on" => {
                // Next token is background color
                if let Some(bg) = tokens.get(i + 1) {
                    style = style.bg(parse_color(bg));
                    i += 1;
                }
            }
            _ if t.starts_with("on:") || t.starts_with("bg:") => {
                if let Some((_, c)) = t.split_once(':') {
                    style = style.bg(parse_color(c));
                }
            }
            _ if t.starts_with("on") && t.len() > 2 => {
                // "oncyan", "on_cyan", "on-cyan"
                let c = t
                    .trim_start_matches("on")
                    .trim_start_matches(['_', '-', ':']);
                if !c.is_empty() {
                    style = style.bg(parse_color(c));
                }
            }
            "black" | "red" | "green" | "yellow" | "blue" | "magenta" | "bright_magenta"
            | "cyan" | "gray" | "grey" | "dark_gray" | "darkgray" | "white" => {
                style = style.fg(parse_color(t));
            }
            _ => {}
        }
        i += 1;
    }
    style
}

pub fn border_type_from_str(s: &str) -> BorderType {
    match s.trim().to_lowercase().as_str() {
        "plain" | "line" => BorderType::Plain,
        "double" => BorderType::Double,
        "thick" => BorderType::Thick,
        _ => BorderType::Rounded,
    }
}

/// Pre-parsed theme styles — built once per App to avoid per-frame `parse_style()` cost.
/// Per ratatui perf: parsing strings per row per frame is wasteful for 50+ rows at 5fps poll.
#[derive(Debug, Clone)]
pub struct ThemeStyles {
    pub border_focused: Style,
    pub border_unfocused: Style,
    pub border_error: Style,
    pub text: Style,
    pub text_dim: Style,
    pub repo_core: Style,
    pub repo_extra: Style,
    pub repo_multilib: Style,
    pub repo_aur: Style,
    pub repo_other: Style,
    pub repo_local: Style,
    pub installed: Style,
    pub out_of_date: Style,
    pub votes: Style,
    pub version: Style,
    pub highlight: Style,
    pub status_loading: Style,
    pub status_idle: Style,
    pub help: Style,
    pub popup_title: Style,
    pub popup_bg: Style,
    pub tab_selected: Style,
    pub tab_normal: Style,
}

impl From<&ThemeConfig> for ThemeStyles {
    fn from(t: &ThemeConfig) -> Self {
        let highlight_bg = parse_color(&t.highlight_bg);
        let fg_style = parse_style(&t.highlight_fg);
        let mut highlight = Style::default().bg(highlight_bg);
        if let Some(c) = fg_style.fg {
            highlight = highlight.fg(c);
        }
        highlight = highlight.add_modifier(fg_style.add_modifier);
        Self {
            border_focused: parse_style(&t.border_focused),
            border_unfocused: parse_style(&t.border_unfocused),
            border_error: parse_style(&t.border_error),
            text: parse_style(&t.text),
            text_dim: parse_style(&t.text_dim),
            repo_core: parse_style(&t.repo_core),
            repo_extra: parse_style(&t.repo_extra),
            repo_multilib: parse_style(&t.repo_multilib),
            repo_aur: parse_style(&t.repo_aur),
            repo_other: parse_style(&t.repo_other),
            repo_local: parse_style(&t.repo_local),
            installed: parse_style(&t.installed),
            out_of_date: parse_style(&t.out_of_date),
            votes: parse_style(&t.votes),
            version: parse_style(&t.version),
            highlight,
            status_loading: parse_style(&t.status_loading),
            status_idle: parse_style(&t.status_idle),
            help: parse_style(&t.help),
            popup_title: parse_style(&t.popup_title),
            popup_bg: parse_style(&t.popup_bg),
            tab_selected: parse_style(&t.tab_selected),
            tab_normal: parse_style(&t.tab_normal),
        }
    }
}

pub fn parse_source(s: &str) -> crate::cli::Source {
    match s.trim().to_lowercase().as_str() {
        "aur" => crate::cli::Source::Aur,
        "repo" => crate::cli::Source::Repo,
        "flatpak" => crate::cli::Source::Flatpak,
        _ => crate::cli::Source::All,
    }
}

pub fn parse_aur_by(s: &str) -> crate::cli::AurBy {
    match s.trim().to_lowercase().as_str() {
        "name" => crate::cli::AurBy::Name,
        "maintainer" => crate::cli::AurBy::Maintainer,
        "depends" => crate::cli::AurBy::Depends,
        "makedepends" => crate::cli::AurBy::Makedepends,
        "optdepends" => crate::cli::AurBy::Optdepends,
        "checkdepends" => crate::cli::AurBy::Checkdepends,
        _ => crate::cli::AurBy::NameDesc,
    }
}

impl Config {
    pub fn effective_source(&self) -> crate::cli::Source {
        parse_source(&self.search.source)
    }
    pub fn effective_aur_by(&self) -> crate::cli::AurBy {
        parse_aur_by(&self.search.by)
    }
    pub fn aur_rpc_url(&self) -> String {
        self.behavior
            .aur_rpc
            .clone()
            .unwrap_or_else(|| "https://aur.archlinux.org/rpc/v5".into())
    }
    pub fn cache_dir_path(&self) -> Option<PathBuf> {
        self.behavior.cache_dir.as_ref().map(PathBuf::from)
    }
    /// Validated pacman remove flag, e.g. "Rs" -> "-Rs". Allowlist prevents injection
    /// via config file. Allowed: R, Rs, Rns, Ru, Rsu, Rn, Rc, Rdd (expert).
    pub fn remove_flag_arg(&self) -> String {
        let f = self.behavior.remove_flags.trim().trim_start_matches('-');
        match f {
            "R" | "Rs" | "Rns" | "Ru" | "Rsu" | "Rn" | "Rc" | "Rdd" => format!("-{f}"),
            _ => "-Rs".into(),
        }
    }
    pub fn remove_noconfirm(&self) -> bool {
        self.behavior.remove_noconfirm || std::env::var("PAKMAP_NOCONFIRM").is_ok()
    }
    pub fn refresh_noconfirm(&self) -> bool {
        self.behavior.refresh_noconfirm || std::env::var("PAKMAP_NOCONFIRM").is_ok()
    }
    pub fn upgrade_noconfirm(&self) -> bool {
        self.behavior.upgrade_noconfirm || std::env::var("PAKMAP_NOCONFIRM").is_ok()
    }
    pub fn is_readonly(&self) -> bool {
        self.behavior.readonly
    }
    pub fn clean_noconfirm(&self) -> bool {
        self.behavior.clean_noconfirm || std::env::var("PAKMAP_NOCONFIRM").is_ok()
    }
    /// Effective keep count: CLI --keep wins when non-default, else config.
    /// Clamped to >= 1 — keep 0 would nuke the whole cache.
    pub fn effective_keep(&self, cli_keep: u32) -> u32 {
        let base = if cli_keep != 2 {
            cli_keep
        } else {
            self.behavior.clean_keep
        };
        base.max(1)
    }
    pub fn news_enabled(&self) -> bool {
        self.behavior.news_enabled
    }
    /// P1 flatpak seam: backend enabled (config; binary presence checked
    /// separately via `sources::has_flatpak()` so disabled/off-Arch is
    /// fail-open empty).
    pub fn flatpak_enabled(&self) -> bool {
        self.behavior.flatpak_enabled
    }
    /// P1 flatpak seam: remote name, `flathub` unless overridden with a
    /// non-empty `[behavior] flatpak_remote`.
    pub fn flatpak_remote_name(&self) -> String {
        self.behavior
            .flatpak_remote
            .clone()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "flathub".to_string())
    }
    /// P3: `-y` for flatpak install/remove/clean (config or env, same as the
    /// pacman noconfirm flags).
    pub fn flatpak_noconfirm(&self) -> bool {
        self.behavior.flatpak_noconfirm || std::env::var("PAKMAP_NOCONFIRM").is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_roundtrips() {
        let cfg = Config::default();
        let s = toml::to_string(&cfg).unwrap();
        let back: Config = toml::from_str(&s).unwrap();
        assert_eq!(back.search.limit, 50);
        assert_eq!(back.tui.floor_width, 60);
    }

    #[test]
    fn parse_style_basic() {
        let st = parse_style("cyan bold");
        assert_eq!(st.fg, Some(Color::Cyan));
        assert!(st.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn flatpak_defaults_and_remote_override() {
        let cfg = Config::default();
        assert!(cfg.flatpak_enabled());
        assert_eq!(cfg.flatpak_remote_name(), "flathub");
        let mut custom = Config::default();
        custom.behavior.flatpak_remote = Some("  myremote  ".into());
        assert_eq!(custom.flatpak_remote_name(), "myremote");
        custom.behavior.flatpak_remote = Some("   ".into());
        assert_eq!(custom.flatpak_remote_name(), "flathub");
        custom.behavior.flatpak_enabled = false;
        assert!(!custom.flatpak_enabled());
    }

    #[test]
    fn flatpak_noconfirm_gate() {
        let mut cfg = Config::default();
        assert!(!cfg.flatpak_noconfirm());
        cfg.behavior.flatpak_noconfirm = true;
        assert!(cfg.flatpak_noconfirm());
    }

    #[test]
    fn invalid_toml_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("bad.toml");
        std::fs::write(&p, "[[broken").unwrap();
        assert!(Config::load_from(&p).is_none());
    }

    #[test]
    fn empty_strings_keep_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c.toml");
        std::fs::write(&p, "[search]\nsource=\"\"\nby=\"\"\n").unwrap();
        let cfg = Config::load_from(&p).unwrap();
        assert_eq!(cfg.search.source, "all");
        assert_eq!(cfg.search.by, "name-desc");
    }

    // ---- v0.9.3 Phase 3: project-local config isolation -------------------

    fn project_with_behavior() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("pakmap.toml");
        std::fs::write(
            &p,
            concat!(
                "[behavior]\n",
                "aur_rpc = \"http://attacker.example/rpc\"\n",
                "cache_dir = \"/tmp/evil\"\n",
                "readonly = true\n",
                "makepkg_noconfirm = true\n",
                "flatpak_remote = \"evil-repo\"\n",
                "[tui]\n",
                "border = \"double\"\n",
            ),
        )
        .unwrap();
        (dir, p)
    }

    #[test]
    fn project_behavior_keys_are_ignored_by_default() {
        let (_dir, p) = project_with_behavior();
        let cfg = Config::load_project_checked(&p, false).unwrap();
        // Every [behavior] shape that could redirect execution is stripped:
        // network target, cache location, privilege gates, non-interactivity.
        assert_eq!(cfg.behavior.aur_rpc, None);
        assert_eq!(cfg.behavior.cache_dir, None);
        assert!(!cfg.behavior.readonly);
        assert!(!cfg.behavior.makepkg_noconfirm);
        assert_eq!(cfg.behavior.flatpak_remote, None);
        // Innocent sections still apply (project styling stays personal).
        assert_eq!(cfg.tui.border, "double");
    }

    #[test]
    fn project_with_no_behavior_section_loads_silent_and_complete() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("pakmap.toml");
        std::fs::write(&p, "[theme]\nno_color = true\n").unwrap();
        let cfg = Config::load_project_checked(&p, false).unwrap();
        assert!(cfg.theme.no_color);
        // Default behavior intact when nothing was stripped.
        assert!(!cfg.behavior.readonly);
    }

    #[test]
    fn trusted_opt_in_restores_wholesale_precedence() {
        let (_dir, p) = project_with_behavior();
        let cfg = Config::load_project_checked(&p, true).unwrap();
        assert_eq!(
            cfg.behavior.aur_rpc,
            Some("http://attacker.example/rpc".to_string()),
            "PAKMAP_TRUST_PROJECT=1 parity: the user opted in explicitly"
        );
        assert!(cfg.behavior.readonly);
    }

    #[test]
    fn symlinked_and_special_project_files_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.toml");
        std::fs::write(&target, "[behavior]\nreadonly = true\n").unwrap();
        // Symlink → config could be retargeted at will → ignore the file.
        let link = dir.path().join("pakmap.toml");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(Config::load_project_checked(&link, false).is_none());
        // Even in trusted mode a symlink stays refused (the guard runs first).
        assert!(Config::load_project_checked(&link, true).is_none());
        // Not a regular file (device): refused.
        assert!(Config::load_project_checked(Path::new("/dev/null"), false).is_none());
    }

    #[test]
    fn behavior_header_matches_exactly_the_family() {
        assert!(Config::is_behavior_header("[behavior]"));
        assert!(Config::is_behavior_header("[[behavior]]"));
        assert!(Config::is_behavior_header("[behavior.sub]"));
        assert!(Config::is_behavior_header("[behavior.x.y]"));
        assert!(Config::is_behavior_header("  [behavior]"));
        // Quoted-key spellings are legal headers too.
        assert!(Config::is_behavior_header("[\"behavior\"]"));
        assert!(Config::is_behavior_header("['behavior']"));
        // Neighboring names, inline tables and comments are NOT the section.
        assert!(!Config::is_behavior_header("[behaviors]"));
        assert!(!Config::is_behavior_header("[behavioral]"));
        assert!(!Config::is_behavior_header("[behavior_x]"));
        assert!(!Config::is_behavior_header("# [behavior] (comment)"));
        assert!(!Config::is_behavior_header("aur_rpc = \"[behavior]\""));
        assert!(!Config::is_behavior_header(""));
    }

    #[test]
    fn strip_removes_whole_behavior_sections_only() {
        let src = "[theme]\naccent = \"cyan\"\n\
                   [behavior]\naur_rpc = \"http://evil\"\nlist = [\n  1,\n  2,\n]\n\
                   [[behavior]]\naur_rpc = \"http://evil2\"\n\
                   [search]\nlimit = 7\n";
        let (out, removed) = Config::strip_behavior_section(src);
        assert!(removed);
        // Non-behavior sections survive byte-for-byte…
        assert!(out.contains("[theme]\naccent = \"cyan\"\n"));
        assert!(out.contains("[search]\nlimit = 7\n"));
        // …and no behavior content does, including its multi-line array.
        assert!(!out.contains("evil"));
        assert_eq!(out.matches("behavior]").count(), 0);
    }

    #[test]
    fn strip_quoted_key_headers_too() {
        let src = "[theme]\naccent = \"cyan\"\n[\"behavior\"]\naur_rpc = \"http://evil\"\n";
        let (out, removed) = Config::strip_behavior_section(src);
        assert!(removed);
        assert!(!out.contains("evil"));
    }

    #[test]
    fn strip_handles_behavior_at_eof_and_no_behavior_files() {
        // Section at EOF (no following header): fully dropped.
        let (out, removed) =
            Config::strip_behavior_section("[search]\nlimit = 1\n[behavior]\naur_rpc = \"x\"\n");
        assert!(removed);
        assert!(out.starts_with("[search]\nlimit = 1\n"));
        // No behavior at all: unchanged, quiet.
        let src = "[theme]\naccent = \"cyan\"\n";
        let (out, removed) = Config::strip_behavior_section(src);
        assert!(!removed);
        assert_eq!(out, src);
    }

    #[test]
    fn effective_keep_clamps_and_prefers_cli() {
        let mut cfg = Config::default();
        assert_eq!(cfg.effective_keep(2), 2);
        assert_eq!(cfg.effective_keep(1), 1);
        assert_eq!(cfg.effective_keep(0), 1);
        cfg.behavior.clean_keep = 3;
        assert_eq!(cfg.effective_keep(2), 3);
        assert_eq!(cfg.effective_keep(1), 1);
        assert!(!cfg.clean_noconfirm());
        cfg.behavior.clean_noconfirm = true;
        assert!(cfg.clean_noconfirm());
    }

    #[test]
    fn parse_style_with_bg() {
        let st = parse_style("black on cyan bold");
        assert_eq!(st.fg, Some(Color::Black));
        assert_eq!(st.bg, Some(Color::Cyan));
        assert!(st.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn remove_flag_allowlist() {
        let mut cfg = Config::default();
        assert_eq!(cfg.remove_flag_arg(), "-Rs");
        cfg.behavior.remove_flags = "Rns".into();
        assert_eq!(cfg.remove_flag_arg(), "-Rns");
        cfg.behavior.remove_flags = "--noconfirm; rm -rf".into();
        assert_eq!(cfg.remove_flag_arg(), "-Rs");
    }

    #[test]
    fn theme_styles_cache_builds() {
        let cfg = Config::default();
        let styles = ThemeStyles::from(&cfg.theme);
        assert_eq!(styles.text.fg, Some(Color::White));
        assert_eq!(styles.tab_selected.bg, Some(Color::Cyan));
    }

    #[test]
    fn refresh_noconfirm_when_enabled() {
        let mut cfg = Config::default();
        cfg.behavior.refresh_noconfirm = true;
        assert!(cfg.refresh_noconfirm());
    }

    #[test]
    fn refresh_command_args_respect_noconfirm() {
        let cfg = Config::default();
        let args = crate::install::refresh::refresh_command_args(&cfg);
        assert_eq!(args, vec!["pacman", "-Sy"]);
        let mut cfg_nc = Config::default();
        cfg_nc.behavior.refresh_noconfirm = true;
        let args_nc = crate::install::refresh::refresh_command_args(&cfg_nc);
        assert_eq!(args_nc, vec!["pacman", "-Sy", "--noconfirm"]);
    }
}
