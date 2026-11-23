//! The TUI style registry (overhaul phase 1, plan Phase 1): the one prod-code
//! owner of ratatui `Color::`/`Modifier::`/`Style::` construction in the TUI
//! modules. Everything `ui.rs` paints resolves through [`ThemeStyles`], either
//! the catalog-resolved colored palette (per `[theme]` keys, built once per
//! App) or the NO_COLOR-aware monochrome pair — never an ad-hoc literal.
//!
//! Semantics preserved from v0.10.0 (behavior freeze, plan D2): the mono
//! entries reproduce the old `if app.no_color { … } else { app.styles.X }`
//! fallbacks byte-for-byte, and the one-way `no_color` rule keeps living in
//! the app layer (a file write can never re-enable color after `--no-color` /
//! `NO_COLOR`). The settings preview strip is the deliberate colored-forever
//! consumer: it renders the *draft* palette via [`ThemeStyles::colored`],
//! ignoring `no_color` by design — palette keys edit what the strip shows.

use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::BorderType;

// ---- catalog string → style parsers ----------------------------------------

pub fn parse_color(s: &str) -> Color {
    let t = s.trim();
    match t.to_lowercase().as_str() {
        "black" => return Color::Black,
        "red" => return Color::Red,
        "green" => return Color::Green,
        "yellow" => return Color::Yellow,
        "blue" => return Color::Blue,
        "magenta" | "bright_magenta" => return Color::Magenta,
        "cyan" => return Color::Cyan,
        "gray" | "grey" => return Color::Gray,
        "dark_gray" | "darkgray" | "dark-grey" => return Color::DarkGray,
        "white" => return Color::White,
        "reset" => return Color::Reset,
        _ => {}
    }
    // 0.12 (TUI phase 1): truecolor `#RRGGBB` and 256-color index `0..=255`.
    // Anything malformed falls back to Reset exactly like an unknown name.
    if let Some(hex) = t.strip_prefix('#') {
        if hex.len() == 6
            && let Ok(v) = u32::from_str_radix(hex, 16)
        {
            return Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8);
        }
        return Color::Reset;
    }
    if let Ok(n) = t.parse::<u16>()
        && n <= 255
    {
        return Color::Indexed(n as u8);
    }
    Color::Reset
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
            // 0.12 (TUI phase 1): hex / 256-index foreground tokens.
            _ if t.starts_with('#') || t.parse::<u16>().is_ok_and(|n| n <= 255) => {
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

// ---- the registry -----------------------------------------------------------

/// Pre-parsed theme styles — built once per App to avoid per-frame `parse_style()` cost.
/// Per ratatui perf: parsing strings per row per frame is wasteful for 50+ rows at 5fps poll.
///
/// Two palettes share this shape:
/// - [`ThemeStyles::colored`]: every entry from its catalog key
///   (`theme.<field>` via `parse_style` / `parse_color`).
/// - [`ThemeStyles::monochrome`]: the NO_COLOR pair (`mono: true`), each entry
///   carrying exactly the emphasis the old inline fallbacks drew — concrete
///   shapes (BOLD/REVERSED/…), never color.
///
/// `mono: bool` also drives the composite helpers ([`ThemeStyles::emphasized`],
/// [`ThemeStyles::preview_banner`]) where colored and mono behavior diverge.
#[derive(Debug, Clone)]
pub struct ThemeStyles {
    /// true = the NO_COLOR-aware pair; false = the colored catalog palette.
    pub mono: bool,
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
    pub status_success: Style,
    pub status_warn: Style,
    pub status_error: Style,
    pub help: Style,
    pub popup_title: Style,
    pub popup_bg: Style,
    pub tab_selected: Style,
    pub tab_normal: Style,
}

impl From<&crate::config::ThemeConfig> for ThemeStyles {
    fn from(t: &crate::config::ThemeConfig) -> Self {
        Self::colored(t)
    }
}

impl ThemeStyles {
    /// The colored catalog palette (identical to the pre-registry v0.10 build).
    pub fn colored(t: &crate::config::ThemeConfig) -> Self {
        let highlight_bg = parse_color(&t.highlight_bg);
        let fg_style = parse_style(&t.highlight_fg);
        let mut highlight = Style::default().bg(highlight_bg);
        if let Some(c) = fg_style.fg {
            highlight = highlight.fg(c);
        }
        highlight = highlight.add_modifier(fg_style.add_modifier);
        Self {
            mono: false,
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
            status_success: parse_style(&t.status_success),
            status_warn: parse_style(&t.status_warn),
            status_error: parse_style(&t.status_error),
            help: parse_style(&t.help),
            popup_title: parse_style(&t.popup_title),
            popup_bg: parse_style(&t.popup_bg),
            tab_selected: parse_style(&t.tab_selected),
            tab_normal: parse_style(&t.tab_normal),
        }
    }

    /// The NO_COLOR-aware pair. Byte-faithful to the v0.10 inline fallbacks:
    /// emphasis becomes modifier-only (BOLD/REVERSED), everything else drops
    /// to the terminal default.
    pub fn monochrome() -> Self {
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let reversed = Style::default().add_modifier(Modifier::REVERSED);
        Self {
            mono: true,
            border_focused: Style::default(),
            border_unfocused: Style::default(),
            border_error: bold,
            text: Style::default(),
            text_dim: Style::default(),
            repo_core: bold,
            repo_extra: bold,
            repo_multilib: bold,
            repo_aur: bold,
            repo_other: bold,
            repo_local: bold,
            installed: bold,
            out_of_date: bold,
            votes: Style::default(),
            version: Style::default(),
            highlight: reversed,
            status_loading: Style::default(),
            status_idle: Style::default(),
            status_success: bold,
            status_warn: bold,
            status_error: bold,
            help: Style::default(),
            popup_title: Style::default(),
            popup_bg: Style::default(),
            tab_selected: Style::default()
                .add_modifier(Modifier::REVERSED)
                .add_modifier(Modifier::BOLD),
            tab_normal: Style::default(),
        }
    }

    /// Label emphasis (info-popup field labels): colored keeps its base entry
    /// and adds BOLD; mono reduces to bare BOLD.
    pub fn emphasized(&self, base: Style) -> Style {
        base.add_modifier(Modifier::BOLD)
    }

    /// The settings-modal preview banner: mono pops with BOLD over default,
    /// colored stays the plain `text` entry.
    pub fn preview_banner(&self) -> Style {
        if self.mono {
            self.emphasized(self.text)
        } else {
            self.text
        }
    }

    /// A draft value that differs from the stored file — REVERSED in both
    /// palettes (it must pop without depending on theme colors).
    pub fn draft_changed(&self) -> Style {
        Style::default().add_modifier(Modifier::REVERSED)
    }

    /// The remove-dialog border: colored uses the `out_of_date` entry, mono
    /// drops to the terminal default (the v0.10 mono path drew no border
    /// emphasis — fidelity preserved, phase 1).
    pub fn danger_border(&self) -> Style {
        if self.mono {
            Style::default()
        } else {
            self.out_of_date
        }
    }

    /// The message-dialog border: colored distinguishes `✓` (installed)
    /// from failure (`out_of_date`); mono stays the bare default (v0.10
    /// fidelity, phase 1).
    pub fn message_border(&self, ok: bool) -> Style {
        if self.mono {
            Style::default()
        } else if ok {
            self.installed
        } else {
            self.out_of_date
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    // ---- moved from config.rs (registry owns the parsers) -------------------

    #[test]
    fn parse_style_basic() {
        let st = parse_style("cyan bold");
        assert_eq!(st.fg, Some(Color::Cyan));
        assert!(st.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn parse_style_with_bg() {
        let st = parse_style("black on cyan bold");
        assert_eq!(st.fg, Some(Color::Black));
        assert_eq!(st.bg, Some(Color::Cyan));
        assert!(st.add_modifier.contains(Modifier::BOLD));
    }

    // ---- 0.12 (TUI phase 1): truecolor + 256-index parsing ------------------

    #[test]
    fn parse_color_accepts_hex_and_index() {
        assert_eq!(parse_color("#ff8800"), Color::Rgb(255, 136, 0));
        assert_eq!(parse_color("#000000"), Color::Rgb(0, 0, 0));
        assert_eq!(parse_color("208"), Color::Indexed(208));
        assert_eq!(parse_color("0"), Color::Indexed(0));
        assert_eq!(parse_color("255"), Color::Indexed(255));
        // malformed / out-of-range fall back to Reset, never panic.
        assert_eq!(parse_color("#xyz"), Color::Reset);
        assert_eq!(parse_color("#fff"), Color::Reset);
        assert_eq!(parse_color("256"), Color::Reset);
        assert_eq!(parse_color("-1"), Color::Reset);
        assert_eq!(parse_color("nope"), Color::Reset);
    }

    #[test]
    fn parse_style_accepts_hex_and_index_fg_bg() {
        let st = parse_style("#ff8800 on #101010 bold");
        assert_eq!(st.fg, Some(Color::Rgb(255, 136, 0)));
        assert_eq!(st.bg, Some(Color::Rgb(16, 16, 16)));
        assert!(st.add_modifier.contains(Modifier::BOLD));
        assert_eq!(parse_style("208").fg, Some(Color::Indexed(208)));
        assert_eq!(parse_style("white on 236").bg, Some(Color::Indexed(236)));
    }

    // ---- 0.12 (TUI phase 1): default-palette contrast audit -----------------
    //
    // ANSI names are terminal-dependent; this audits the shipped defaults
    // against the xterm reference RGB for the 16 base colors (documented in
    // docs/tui.md). It pins the *choice* of default names, not a claim of
    // universal WCAG conformance on every emulator/theme.

    fn reference_rgb(c: Color) -> (u8, u8, u8) {
        match c {
            Color::Black => (0, 0, 0),
            Color::Red => (205, 0, 0),
            Color::Green => (0, 205, 0),
            Color::Yellow => (205, 205, 0),
            Color::Blue => (0, 0, 238),
            Color::Magenta => (205, 0, 205),
            Color::Cyan => (0, 205, 205),
            Color::Gray => (229, 229, 229),
            Color::DarkGray => (85, 85, 85),
            Color::White => (255, 255, 255),
            Color::Rgb(r, g, b) => (r, g, b),
            _ => (0, 0, 0),
        }
    }

    fn luminance(c: Color) -> f64 {
        let (r, g, b) = reference_rgb(c);
        let lin = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
    }

    fn contrast(fg: Color, bg: Color) -> f64 {
        let a = luminance(fg);
        let b = luminance(bg);
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn default_palette_meets_wcag_contrast_on_black() {
        let t = crate::config::ThemeConfig::default();
        let black = Color::Black;
        let fg = |s: &str| parse_style(s).fg.unwrap_or(Color::Reset);
        // Primary text: >= 4.5:1.
        assert!(contrast(fg(&t.text), black) >= 4.5, "text");
        assert!(contrast(fg(&t.text_dim), black) >= 4.5, "text_dim");
        assert!(contrast(fg(&t.version), black) >= 4.5, "version");
        // Secondary text / UI components: >= 3:1.
        assert!(contrast(fg(&t.help), black) >= 3.0, "help");
        assert!(contrast(fg(&t.tab_normal), black) >= 3.0, "tab_normal");
        assert!(
            contrast(fg(&t.border_unfocused), black) >= 3.0,
            "border_unfocused"
        );
        assert!(
            contrast(fg(&t.border_focused), black) >= 3.0,
            "border_focused"
        );
        assert!(contrast(fg(&t.status_idle), black) >= 3.0, "status_idle");
        for (name, style) in [
            ("status_success", parse_style(&t.status_success)),
            ("status_warn", parse_style(&t.status_warn)),
            ("status_error", parse_style(&t.status_error)),
        ] {
            assert!(
                contrast(style.fg.unwrap_or(Color::Reset), black) >= 3.0,
                "{name}"
            );
        }
        // Composite pairs: selected row + active tab keep >= 4.5:1.
        let hl_bg = parse_color(&t.highlight_bg);
        assert!(contrast(fg(&t.highlight_fg), hl_bg) >= 4.5, "highlight");
        let tab = parse_style(&t.tab_selected);
        assert!(
            contrast(tab.fg.unwrap_or(Color::Reset), tab.bg.unwrap_or(black)) >= 4.5,
            "tab_selected"
        );
        // Regression guard: the old dark_gray dim roles failed the 3:1 floor.
        assert!(contrast(Color::DarkGray, black) < 3.0);
    }

    #[test]
    fn theme_styles_cache_builds() {
        let cfg = Config::default();
        let styles = ThemeStyles::colored(&cfg.theme);
        assert_eq!(styles.text.fg, Some(Color::White));
        assert_eq!(styles.tab_selected.bg, Some(Color::Cyan));
    }

    // ---- phase 1: palette-catalog parity + mono fidelity ---------------------

    /// Every colored entry maps to its catalog key (`theme.<field>`); the
    /// composite `highlight` rides `theme.highlight_fg` + `theme.highlight_bg`.
    #[test]
    fn palette_fields_match_catalog_theme_keys() {
        let string_fields = [
            "border_focused",
            "border_unfocused",
            "border_error",
            "text",
            "text_dim",
            "repo_core",
            "repo_extra",
            "repo_multilib",
            "repo_aur",
            "repo_other",
            "repo_local",
            "installed",
            "out_of_date",
            "votes",
            "version",
            "highlight_fg",
            "highlight_bg",
            "status_loading",
            "status_idle",
            "status_success",
            "status_warn",
            "status_error",
            "help",
            "popup_title",
            "popup_bg",
            "tab_selected",
            "tab_normal",
        ];
        for field in string_fields {
            let key = format!("theme.{field}");
            assert!(
                crate::settings::catalog::CATALOG
                    .iter()
                    .any(|m| m.key == key),
                "ThemeStyles field `{field}` has no catalog key `{key}`"
            );
        }
    }

    /// The mono pair must keep drawing exactly what the v0.10 inline
    /// fallbacks drew — pin each entry's concrete shape: colorless, with
    /// BOLD only where the old else-branches said BOLD, REVERSED for the
    /// list highlight, REVERSED|BOLD for the active tab.
    #[test]
    fn monochrome_matches_the_v010_inline_fallbacks() {
        let m = ThemeStyles::monochrome();
        assert!(m.mono);
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let reversed = Style::default().add_modifier(Modifier::REVERSED);
        let tab = Style::default()
            .add_modifier(Modifier::REVERSED)
            .add_modifier(Modifier::BOLD);
        for name in [
            "border_error",
            "repo_core",
            "repo_extra",
            "repo_multilib",
            "repo_aur",
            "repo_other",
            "repo_local",
            "installed",
            "out_of_date",
            "status_success",
            "status_warn",
            "status_error",
        ] {
            let st = match name {
                "border_error" => m.border_error,
                "repo_core" => m.repo_core,
                "repo_extra" => m.repo_extra,
                "repo_multilib" => m.repo_multilib,
                "repo_aur" => m.repo_aur,
                "repo_other" => m.repo_other,
                "repo_local" => m.repo_local,
                "installed" => m.installed,
                "out_of_date" => m.out_of_date,
                "status_success" => m.status_success,
                "status_warn" => m.status_warn,
                _ => m.status_error,
            };
            assert_eq!(st, bold, "mono.{name} must draw the bare-BOLD fallback");
        }
        assert_eq!(m.highlight, reversed);
        assert_eq!(m.tab_selected, tab);
        for name in [
            "border_focused",
            "border_unfocused",
            "text",
            "text_dim",
            "votes",
            "version",
            "status_loading",
            "status_idle",
            "help",
            "popup_title",
            "popup_bg",
            "tab_normal",
        ] {
            let st = match name {
                "border_focused" => m.border_focused,
                "border_unfocused" => m.border_unfocused,
                "text" => m.text,
                "text_dim" => m.text_dim,
                "votes" => m.votes,
                "version" => m.version,
                "status_loading" => m.status_loading,
                "status_idle" => m.status_idle,
                "help" => m.help,
                "popup_title" => m.popup_title,
                "popup_bg" => m.popup_bg,
                _ => m.tab_normal,
            };
            assert_eq!(
                st,
                Style::default(),
                "mono.{name} must stay default — \
                colorless, no modifiers (equality covers fg/bg/emptiness)"
            );
        }
    }

    // ---- phase 1: the grep-enforced D1 acceptance gate -----------------------

    /// D1 acceptance: no TUI module may construct ratatui styles ad hoc —
    /// `Color::`/`Modifier::`/`Style::` live only in this registry file.
    /// Checked against the real source via `include_str!`, pinning prod and
    /// app/test code alike; the registry file itself is excluded by design.
    /// Overhaul phase 2: the drawing tree is now `ui/` — every layer joins
    /// the gate as its own file (the gate stays whole-tree, not per-menu).
    #[test]
    fn ui_modules_delegate_all_styling_to_the_registry() {
        const APP: &str = include_str!("app.rs");
        const SETTINGS: &str = include_str!("settings.rs");
        const MOD: &str = include_str!("mod.rs");
        const KEYS: &str = include_str!("keys.rs");
        const UI_MOD: &str = include_str!("ui/mod.rs");
        const UI_LAYOUT: &str = include_str!("ui/layout.rs");
        const UI_CHROME: &str = include_str!("ui/chrome.rs");
        const UI_BODY: &str = include_str!("ui/body.rs");
        const UI_OVERLAYS: &str = include_str!("ui/overlays.rs");
        const UI_SETTINGS: &str = include_str!("ui/settings_overlay.rs");
        for (file, src) in [
            ("app.rs", APP),
            ("settings.rs", SETTINGS),
            ("mod.rs", MOD),
            ("keys.rs", KEYS),
            ("ui/mod.rs", UI_MOD),
            ("ui/layout.rs", UI_LAYOUT),
            ("ui/chrome.rs", UI_CHROME),
            ("ui/body.rs", UI_BODY),
            ("ui/overlays.rs", UI_OVERLAYS),
            ("ui/settings_overlay.rs", UI_SETTINGS),
        ] {
            for token in ["Color::", "Modifier::", "Style::"] {
                assert!(
                    !src.contains(token),
                    "{file} contains `{token}` — routing styling through \
                     src/tui/style.rs is the D1 contract; fix the file, not \
                     the test"
                );
            }
        }
    }

    // ---- phase 1 helpers ------------------------------------------------------

    #[test]
    fn helper_semantics_diverge_only_where_the_v010_fallbacks_did() {
        let colored = ThemeStyles::colored(&Config::default().theme);
        let mono = ThemeStyles::monochrome();
        // emphasized: base+BOLD both ways (mono base is default → bare BOLD).
        assert_eq!(
            mono.emphasized(mono.status_loading),
            Style::default().add_modifier(Modifier::BOLD)
        );
        assert_eq!(colored.emphasized(colored.text).fg, Some(Color::White));
        // preview banner: colored plain text; mono bare BOLD over default.
        assert_eq!(colored.preview_banner(), colored.text);
        assert_eq!(
            mono.preview_banner(),
            Style::default().add_modifier(Modifier::BOLD)
        );
        // draft changed: REVERSED in both palettes, theme-independent.
        assert_eq!(mono.draft_changed(), colored.draft_changed());
        // the two fidelity-divergent borders (mono = bare default)
        assert_eq!(mono.danger_border(), Style::default());
        assert_eq!(colored.danger_border(), colored.out_of_date);
        assert_eq!(mono.message_border(true), Style::default());
        assert_eq!(colored.message_border(true), colored.installed);
        assert_eq!(colored.message_border(false), colored.out_of_date);
    }
}
