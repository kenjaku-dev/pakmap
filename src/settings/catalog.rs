//! The settings catalog: one entry per canonical `Config` key (plan §1.1/§1.2).
//!
//! Every editable key of [`crate::config::Config`] appears exactly once here —
//! in canonical (non-alias) spelling. `[behavior].bottom_up` is intentionally
//! NOT an entry: it stays accepted as a deprecated alias of `search.bottom_up`
//! (plan §1.2); the resolver carries the alias policy.
//!
//! Environment rows are catalog-adjacent, not catalog: they are not `Config`
//! fields (parity test pins that), so `list`/`docs` surface them from
//! [`ENV_ROWS`] as read-only info rows (plan §1.1 EnvOnly).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Bool,
    U16,
    U32,
    U64,
    Usize,
    Enum(&'static [EnumVal]),
    Style,
    Text,
    OptText,
    PairU16,
}

/// One allowed value of a `Kind::Enum` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnumVal {
    pub name: &'static str,
    pub desc: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Search,
    BehaviorSafety,
    NetworkCache,
    UpdatesNews,
    Flatpak,
    Appearance,
    Advanced,
    Gui,
    EnvOnly,
}

impl Group {
    /// Sidebar label (plan §3 groups; GUI phase renders from this).
    pub fn as_label(&self) -> &'static str {
        match self {
            Group::Search => "Search",
            Group::BehaviorSafety => "Behavior & safety",
            Group::NetworkCache => "Network & cache",
            Group::UpdatesNews => "Updates & news",
            Group::Flatpak => "Flatpak",
            Group::Appearance => "Appearance",
            Group::Advanced => "Advanced",
            Group::Gui => "GUI",
            Group::EnvOnly => "Environment",
        }
    }
}

/// UI risk level of a row (plan §3: danger rows always visible).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Danger {
    None,
    Caution,
    Danger,
}

/// Which doors can surface a key (edit surface, not visibility).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Surfaces {
    pub cli: bool,
    pub tui: bool,
    pub gui: bool,
}

/// When a changed value takes effect (plan §1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Apply {
    Immediate,
    NextLaunch,
    LivePreview,
}

impl Apply {
    pub fn as_str(&self) -> &'static str {
        match self {
            Apply::Immediate => "immediate",
            Apply::NextLaunch => "next-launch",
            Apply::LivePreview => "live-preview",
        }
    }
}

impl Danger {
    pub fn as_str(&self) -> &'static str {
        match self {
            Danger::None => "",
            Danger::Caution => "caution",
            Danger::Danger => "danger",
        }
    }
}

/// One catalog row: declaration of exactly one canonical setting key.
#[derive(Debug, Clone, Copy)]
pub struct SettingMeta {
    /// Canonical dotted key, e.g. `search.limit`.
    pub key: &'static str,
    /// Which `Config` field (exhaustive enum keeps parity forced, plan §1.2).
    pub field: super::FieldId,
    pub kind: Kind,
    /// Default rendered as a set()-parsable string.
    pub default: &'static str,
    pub group: Group,
    pub danger: Danger,
    pub surfaces: Surfaces,
    pub apply: Apply,
    /// One English line (GUI is English-UI, plan §1.1).
    pub desc: &'static str,
}

impl SettingMeta {
    pub fn lookup(key: &str) -> Option<&'static SettingMeta> {
        CATALOG.iter().find(|m| m.key == key)
    }
}

pub const SEARCH_SOURCE_VALUES: &[EnumVal] = &[
    EnumVal {
        name: "all",
        desc: "official repos + AUR + flatpak",
    },
    EnumVal {
        name: "aur",
        desc: "AUR only",
    },
    EnumVal {
        name: "repo",
        desc: "official repos only",
    },
    EnumVal {
        name: "flatpak",
        desc: "flatpak only",
    },
];

pub const AUR_BY_VALUES: &[EnumVal] = &[
    EnumVal {
        name: "name-desc",
        desc: "name and description",
    },
    EnumVal {
        name: "name",
        desc: "package name",
    },
    EnumVal {
        name: "maintainer",
        desc: "AUR maintainer",
    },
    EnumVal {
        name: "depends",
        desc: "runtime depends",
    },
    EnumVal {
        name: "makedepends",
        desc: "make-time depends",
    },
    EnumVal {
        name: "optdepends",
        desc: "optional depends",
    },
    EnumVal {
        name: "checkdepends",
        desc: "check-time depends",
    },
];

pub const BORDER_VALUES: &[EnumVal] = &[
    EnumVal {
        name: "rounded",
        desc: "rounded corners",
    },
    EnumVal {
        name: "plain",
        desc: "plain lines",
    },
    EnumVal {
        name: "double",
        desc: "double lines",
    },
    EnumVal {
        name: "thick",
        desc: "thick lines",
    },
];

/// plan D1: theme reach values; `device` reproduces today's exact behavior.
pub const THEME_VALUES: &[EnumVal] = &[
    EnumVal {
        name: "device",
        desc: "the device/webview decides (current behavior)",
    },
    EnumVal {
        name: "system",
        desc: "follow the system light/dark setting",
    },
    EnumVal {
        name: "light",
        desc: "force light palette",
    },
    EnumVal {
        name: "dark",
        desc: "force dark palette",
    },
];

const CLI_ONLY_STYLE: Surfaces = Surfaces {
    cli: true,
    tui: true,
    gui: true,
};
const TUI_STYLE: Surfaces = Surfaces {
    cli: false,
    tui: true,
    gui: true,
};

pub const CATALOG: &[SettingMeta] = &[
    // ---- [search] ----------------------------------------------------------
    SettingMeta {
        key: "search.limit",
        field: super::FieldId::SearchLimit,
        kind: Kind::Usize,
        default: "50",
        group: Group::Search,
        danger: Danger::None,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "max results per source; 0 = no limit",
    },
    SettingMeta {
        key: "search.source",
        field: super::FieldId::SearchSource,
        kind: Kind::Enum(SEARCH_SOURCE_VALUES),
        default: "all",
        group: Group::Search,
        danger: Danger::None,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "where to search: aur|repo|flatpak|all",
    },
    SettingMeta {
        key: "search.by",
        field: super::FieldId::SearchBy,
        kind: Kind::Enum(AUR_BY_VALUES),
        default: "name-desc",
        group: Group::Search,
        danger: Danger::None,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "AUR search field",
    },
    SettingMeta {
        key: "search.regex",
        field: super::FieldId::SearchRegex,
        kind: Kind::Bool,
        default: "false",
        group: Group::Search,
        danger: Danger::None,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "treat queries as regex",
    },
    SettingMeta {
        key: "search.installed_only",
        field: super::FieldId::SearchInstalledOnly,
        kind: Kind::Bool,
        default: "false",
        group: Group::Search,
        danger: Danger::None,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "show only installed packages (repo side)",
    },
    SettingMeta {
        key: "search.timeout_secs",
        field: super::FieldId::SearchTimeoutSecs,
        kind: Kind::U64,
        default: "15",
        group: Group::Search,
        danger: Danger::None,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "AUR/http timeout seconds (clamped 3..=30)",
    },
    // Canonical home of bottom_up. The old [behavior] spelling is accepted
    // as a deprecated alias (plan §1.2) — not a second catalog entry.
    SettingMeta {
        key: "search.bottom_up",
        field: super::FieldId::SearchBottomUp,
        kind: Kind::Bool,
        default: "false",
        group: Group::Search,
        danger: Danger::None,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "print results bottom-up (AUR first); [behavior] bottom_up is a deprecated alias",
    },
    // ---- [tui] --------------------------------------------------------------
    SettingMeta {
        key: "tui.enabled",
        field: super::FieldId::TuiEnabled,
        kind: Kind::Bool,
        default: "true",
        group: Group::Advanced,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::NextLaunch,
        desc: "launch the TUI when stdin/stdout are terminals",
    },
    SettingMeta {
        key: "tui.floor_width",
        field: super::FieldId::TuiFloorWidth,
        kind: Kind::U16,
        default: "60",
        group: Group::Advanced,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::NextLaunch,
        desc: "minimum terminal width for the TUI (columns)",
    },
    SettingMeta {
        key: "tui.floor_height",
        field: super::FieldId::TuiFloorHeight,
        kind: Kind::U16,
        default: "14",
        group: Group::Advanced,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::NextLaunch,
        desc: "minimum terminal height for the TUI (rows)",
    },
    SettingMeta {
        key: "tui.layout_search",
        field: super::FieldId::TuiLayoutSearch,
        kind: Kind::U16,
        default: "3",
        group: Group::Advanced,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::NextLaunch,
        desc: "search bar height (rows)",
    },
    SettingMeta {
        key: "tui.layout_results_min",
        field: super::FieldId::TuiLayoutResultsMin,
        kind: Kind::U16,
        default: "8",
        group: Group::Advanced,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::NextLaunch,
        desc: "minimum results height (rows)",
    },
    SettingMeta {
        key: "tui.border",
        field: super::FieldId::TuiBorder,
        kind: Kind::Enum(BORDER_VALUES),
        default: "rounded",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::NextLaunch,
        desc: "panel border style",
    },
    SettingMeta {
        key: "tui.highlight_symbol",
        field: super::FieldId::TuiHighlightSymbol,
        kind: Kind::OptText,
        default: "",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::NextLaunch,
        desc: "selected-row marker (empty = \"▸ \" default, plain none)",
    },
    SettingMeta {
        key: "tui.show_tabs",
        field: super::FieldId::TuiShowTabs,
        kind: Kind::Bool,
        default: "true",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::NextLaunch,
        desc: "show the Search/Installed tab bar",
    },
    SettingMeta {
        key: "tui.preview_pane",
        field: super::FieldId::TuiPreviewPane,
        kind: Kind::Bool,
        default: "false",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::NextLaunch,
        desc: "show a preview pane next to the list (desktop widths; narrower terminals keep the `i` info modal)",
    },
    SettingMeta {
        key: "tui.popup_info",
        field: super::FieldId::TuiPopupInfo,
        kind: Kind::PairU16,
        default: "70,60",
        group: Group::Advanced,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::NextLaunch,
        desc: "info popup size (cols,rows)",
    },
    SettingMeta {
        key: "tui.popup_confirm",
        field: super::FieldId::TuiPopupConfirm,
        kind: Kind::PairU16,
        default: "60,30",
        group: Group::Advanced,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::NextLaunch,
        desc: "confirm popup size (cols,rows)",
    },
    SettingMeta {
        key: "tui.popup_message",
        field: super::FieldId::TuiPopupMessage,
        kind: Kind::PairU16,
        default: "60,20",
        group: Group::Advanced,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::NextLaunch,
        desc: "message popup size (cols,rows)",
    },
    SettingMeta {
        key: "tui.popup_help",
        field: super::FieldId::TuiPopupHelp,
        kind: Kind::PairU16,
        default: "70,60",
        group: Group::Advanced,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::NextLaunch,
        desc: "help popup size (cols,rows)",
    },
    SettingMeta {
        key: "tui.tick_chars",
        field: super::FieldId::TuiTickChars,
        kind: Kind::OptText,
        default: "",
        group: Group::Advanced,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::NextLaunch,
        desc: "spinner characters (empty = braille default)",
    },
    SettingMeta {
        key: "tui.spinner_template",
        field: super::FieldId::TuiSpinnerTemplate,
        kind: Kind::OptText,
        default: "",
        group: Group::Advanced,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::NextLaunch,
        desc: "indicatif spinner template (empty = \"{spinner:.cyan} {msg}\")",
    },
    SettingMeta {
        key: "tui.debounce_ms",
        field: super::FieldId::TuiDebounceMs,
        kind: Kind::U64,
        default: "400",
        group: Group::Advanced,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::NextLaunch,
        desc: "search-as-you-type debounce ms (clamped 50..=5000)",
    },
    SettingMeta {
        key: "tui.poll_ms",
        field: super::FieldId::TuiPollMs,
        kind: Kind::U64,
        default: "200",
        group: Group::Advanced,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::NextLaunch,
        desc: "event poll interval ms (clamped 50..=1000)",
    },
    // ---- [theme] ------------------------------------------------------------
    SettingMeta {
        key: "theme.no_color",
        field: super::FieldId::ThemeNoColor,
        kind: Kind::Bool,
        default: "false",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::LivePreview,
        desc: "disable colored output (mirrors --no-color)",
    },
    SettingMeta {
        key: "theme.border_focused",
        field: super::FieldId::ThemeBorderFocused,
        kind: Kind::Style,
        default: "cyan",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "focused panel border style",
    },
    SettingMeta {
        key: "theme.border_unfocused",
        field: super::FieldId::ThemeBorderUnfocused,
        kind: Kind::Style,
        default: "dark_gray",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "unfocused panel border style",
    },
    SettingMeta {
        key: "theme.border_error",
        field: super::FieldId::ThemeBorderError,
        kind: Kind::Style,
        default: "red bold",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "error panel border style",
    },
    SettingMeta {
        key: "theme.text",
        field: super::FieldId::ThemeText,
        kind: Kind::Style,
        default: "white",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "regular text style",
    },
    SettingMeta {
        key: "theme.text_dim",
        field: super::FieldId::ThemeTextDim,
        kind: Kind::Style,
        default: "dark_gray",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "dimmed/hint text style",
    },
    SettingMeta {
        key: "theme.repo_core",
        field: super::FieldId::ThemeRepoCore,
        kind: Kind::Style,
        default: "red bold",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "core repo tag style",
    },
    SettingMeta {
        key: "theme.repo_extra",
        field: super::FieldId::ThemeRepoExtra,
        kind: Kind::Style,
        default: "green bold",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "extra repo tag style",
    },
    SettingMeta {
        key: "theme.repo_multilib",
        field: super::FieldId::ThemeRepoMultilib,
        kind: Kind::Style,
        default: "blue bold",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "multilib repo tag style",
    },
    SettingMeta {
        key: "theme.repo_aur",
        field: super::FieldId::ThemeRepoAur,
        kind: Kind::Style,
        default: "magenta bold",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "AUR tag style",
    },
    SettingMeta {
        key: "theme.repo_other",
        field: super::FieldId::ThemeRepoOther,
        kind: Kind::Style,
        default: "cyan bold",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "other repo tag style",
    },
    SettingMeta {
        key: "theme.repo_local",
        field: super::FieldId::ThemeRepoLocal,
        kind: Kind::Style,
        default: "green bold",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "local/pushed repo tag style",
    },
    SettingMeta {
        key: "theme.installed",
        field: super::FieldId::ThemeInstalled,
        kind: Kind::Style,
        default: "green bold",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "installed-package marker style",
    },
    SettingMeta {
        key: "theme.out_of_date",
        field: super::FieldId::ThemeOutOfDate,
        kind: Kind::Style,
        default: "red bold",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "out-of-date marker style",
    },
    SettingMeta {
        key: "theme.votes",
        field: super::FieldId::ThemeVotes,
        kind: Kind::Style,
        default: "yellow",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "AUR vote count style",
    },
    SettingMeta {
        key: "theme.version",
        field: super::FieldId::ThemeVersion,
        kind: Kind::Style,
        default: "white",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "version column style",
    },
    SettingMeta {
        key: "theme.highlight_bg",
        field: super::FieldId::ThemeHighlightBg,
        kind: Kind::Style,
        default: "dark_gray",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "selection background",
    },
    SettingMeta {
        key: "theme.highlight_fg",
        field: super::FieldId::ThemeHighlightFg,
        kind: Kind::Style,
        default: "white bold",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "selection foreground",
    },
    SettingMeta {
        key: "theme.status_loading",
        field: super::FieldId::ThemeStatusLoading,
        kind: Kind::Style,
        default: "cyan",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "loading status-line style",
    },
    SettingMeta {
        key: "theme.status_idle",
        field: super::FieldId::ThemeStatusIdle,
        kind: Kind::Style,
        default: "gray",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "idle status-line style",
    },
    SettingMeta {
        key: "theme.help",
        field: super::FieldId::ThemeHelp,
        kind: Kind::Style,
        default: "dark_gray",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "help/hint line style",
    },
    SettingMeta {
        key: "theme.popup_title",
        field: super::FieldId::ThemePopupTitle,
        kind: Kind::Style,
        default: "yellow",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "popup title style",
    },
    SettingMeta {
        key: "theme.popup_bg",
        field: super::FieldId::ThemePopupBg,
        kind: Kind::Style,
        default: "black",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "popup background",
    },
    SettingMeta {
        key: "theme.tab_selected",
        field: super::FieldId::ThemeTabSelected,
        kind: Kind::Style,
        default: "black on cyan bold",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "selected tab style",
    },
    SettingMeta {
        key: "theme.tab_normal",
        field: super::FieldId::ThemeTabNormal,
        kind: Kind::Style,
        default: "dark_gray",
        group: Group::Appearance,
        danger: Danger::None,
        surfaces: TUI_STYLE,
        apply: Apply::LivePreview,
        desc: "inactive tab style",
    },
    // ---- [behavior] ---------------------------------------------------------
    SettingMeta {
        key: "behavior.verbose",
        field: super::FieldId::BehaviorVerbose,
        kind: Kind::Usize,
        default: "0",
        group: Group::Advanced,
        danger: Danger::None,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "verbosity level 0..=2 (mirrors -v counts)",
    },
    SettingMeta {
        key: "behavior.aur_rpc",
        field: super::FieldId::BehaviorAurRpc,
        kind: Kind::OptText,
        default: "",
        group: Group::NetworkCache,
        danger: Danger::Danger,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "AUR RPC base URL override (http/https; blank = default) — trust only your own file",
    },
    SettingMeta {
        key: "behavior.cache_dir",
        field: super::FieldId::BehaviorCacheDir,
        kind: Kind::OptText,
        default: "",
        group: Group::NetworkCache,
        danger: Danger::Danger,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::NextLaunch,
        desc: "AUR clone/cache dir override (blank = XDG cache) — must be your own dir",
    },
    SettingMeta {
        key: "behavior.makepkg_noconfirm",
        field: super::FieldId::BehaviorMakepkgNoconfirm,
        kind: Kind::Bool,
        default: "false",
        group: Group::BehaviorSafety,
        danger: Danger::Caution,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "pass --noconfirm to makepkg builds",
    },
    SettingMeta {
        key: "behavior.remove_flags",
        field: super::FieldId::BehaviorRemoveFlags,
        kind: Kind::Text,
        default: "Rs",
        group: Group::BehaviorSafety,
        danger: Danger::Danger,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "pacman remove flags (allowlist R|Rs|Rns|Ru|Rsu|Rn|Rc|Rdd)",
    },
    SettingMeta {
        key: "behavior.remove_noconfirm",
        field: super::FieldId::BehaviorRemoveNoconfirm,
        kind: Kind::Bool,
        default: "false",
        group: Group::BehaviorSafety,
        danger: Danger::Caution,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "add --noconfirm to package removal",
    },
    SettingMeta {
        key: "behavior.refresh_noconfirm",
        field: super::FieldId::BehaviorRefreshNoconfirm,
        kind: Kind::Bool,
        default: "false",
        group: Group::BehaviorSafety,
        danger: Danger::Caution,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "add --noconfirm to database refresh (pacman -Sy)",
    },
    SettingMeta {
        key: "behavior.readonly",
        field: super::FieldId::BehaviorReadonly,
        kind: Kind::Bool,
        default: "false",
        group: Group::BehaviorSafety,
        danger: Danger::Danger,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "refuse install/remove/refresh/upgrade (safe browse mode)",
    },
    SettingMeta {
        key: "behavior.news_enabled",
        field: super::FieldId::BehaviorNewsEnabled,
        kind: Kind::Bool,
        default: "true",
        group: Group::UpdatesNews,
        danger: Danger::None,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "Arch-news intervention guard before upgrade",
    },
    SettingMeta {
        key: "behavior.news_cache_secs",
        field: super::FieldId::BehaviorNewsCacheSecs,
        kind: Kind::U64,
        default: "86400",
        group: Group::UpdatesNews,
        danger: Danger::None,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "news cache TTL seconds",
    },
    SettingMeta {
        key: "behavior.diff_pager",
        field: super::FieldId::BehaviorDiffPager,
        kind: Kind::OptText,
        default: "",
        group: Group::Advanced,
        danger: Danger::None,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "PKGBUILD diff pager: auto|bat|less|cat (blank = auto)",
    },
    SettingMeta {
        key: "behavior.upgrade_noconfirm",
        field: super::FieldId::BehaviorUpgradeNoconfirm,
        kind: Kind::Bool,
        default: "false",
        group: Group::BehaviorSafety,
        danger: Danger::Caution,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "add --noconfirm to system upgrade",
    },
    SettingMeta {
        key: "behavior.clean_keep",
        field: super::FieldId::BehaviorCleanKeep,
        kind: Kind::U32,
        default: "2",
        group: Group::NetworkCache,
        danger: Danger::None,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "versions kept per package on cache clean (paccache -k, min 1)",
    },
    SettingMeta {
        key: "behavior.clean_clones_days",
        field: super::FieldId::BehaviorCleanClonesDays,
        kind: Kind::Usize,
        default: "0",
        group: Group::NetworkCache,
        danger: Danger::Caution,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "prune AUR clone dirs older than N days on clean (0 = off)",
    },
    SettingMeta {
        key: "behavior.clean_noconfirm",
        field: super::FieldId::BehaviorCleanNoconfirm,
        kind: Kind::Bool,
        default: "false",
        group: Group::BehaviorSafety,
        danger: Danger::Caution,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "non-interactive clean (sudo -n, fails fast)",
    },
    SettingMeta {
        key: "behavior.flatpak_enabled",
        field: super::FieldId::BehaviorFlatpakEnabled,
        kind: Kind::Bool,
        default: "true",
        group: Group::Flatpak,
        danger: Danger::None,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "enable the flatpak backend (fail-open when binary absent)",
    },
    SettingMeta {
        key: "behavior.flatpak_remote",
        field: super::FieldId::BehaviorFlatpakRemote,
        kind: Kind::OptText,
        default: "",
        group: Group::Flatpak,
        danger: Danger::None,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "remote for flatpak install/search (blank = flathub)",
    },
    SettingMeta {
        key: "behavior.flatpak_noconfirm",
        field: super::FieldId::BehaviorFlatpakNoconfirm,
        kind: Kind::Bool,
        default: "false",
        group: Group::Flatpak,
        danger: Danger::Caution,
        surfaces: CLI_ONLY_STYLE,
        apply: Apply::Immediate,
        desc: "pass -y to flatpak install/remove/clean",
    },
    // ---- [gui] --------------------------------------------------------------
    SettingMeta {
        key: "gui.theme",
        field: super::FieldId::GuiTheme,
        kind: Kind::Enum(THEME_VALUES),
        default: "device",
        group: Group::Gui,
        danger: Danger::None,
        surfaces: Surfaces {
            cli: true,
            tui: false,
            gui: true,
        },
        apply: Apply::NextLaunch,
        desc: "GUI theme: device|system|light|dark (device = current behavior)",
    },
];

/// Read-only environment info rows (plan §1.1 EnvOnly): not `Config` fields,
/// shown by `config list`/`docs` so every knob is discoverable.
#[derive(Debug, Clone, Copy)]
pub struct EnvRow {
    pub name: &'static str,
    pub desc: &'static str,
}

pub const ENV_ROWS: &[EnvRow] = &[
    EnvRow {
        name: "PAKMAP_NOCONFIRM",
        desc: "set to 1 to skip all confirmations (makepkg/remove/refresh/upgrade/clean/flatpak)",
    },
    EnvRow {
        name: "PAKMAP_TRUST_PROJECT",
        desc: "set to 1 to trust ./pakmap.toml wholesale, including [behavior] writes",
    },
    EnvRow {
        name: "PAKMAP_SKIP_LIVE",
        desc: "set to 1 to skip live pacman/AUR integration in tests",
    },
    EnvRow {
        name: "PAKMAP_GUI_SMOKE_MS",
        desc: "GUI smoke-boot time budget in ms (CI canary)",
    },
    EnvRow {
        name: "PAKMAP_GUI_FPBUDGET_MS",
        desc: "GUI first-IPC latency budget in ms (0/unset = no budget)",
    },
];

/// Section of a dotted key (`"search.limit"` → `"search"`).
pub fn section_of(key: &str) -> &str {
    key.split('.').next().unwrap_or(key)
}

/// Render a default string as the TOML literal used in the generated example.
/// `default` values are already set()-parsable strings per kind.
fn toml_literal(kind: Kind, value: &str) -> String {
    match kind {
        Kind::Bool => {
            if value == "true" {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        Kind::U16 | Kind::U32 | Kind::U64 | Kind::Usize => value.to_string(),
        Kind::Enum(_) | Kind::Style | Kind::Text | Kind::OptText => {
            format!("{value:?}")
        }
        Kind::PairU16 => {
            let parts = value.split(',').map(str::trim).collect::<Vec<_>>();
            format!("[{}]", parts.join(", "))
        }
    }
}

/// Section-level editorial comments for the generated example file.
const SECTION_NOTES: &[(&str, &[&str])] = &[
    ("search", &[]),
    (
        "tui",
        &["# numbers are columns/rows; floor_* is the TUI minimum size"],
    ),
    (
        "theme",
        &["# styles: \"fg [on bg] [modifiers]\", e.g. \"black on cyan bold\""],
    ),
    (
        "behavior",
        &[
            "# PAKMAP_NOCONFIRM=1 in the environment also enables the noconfirm",
            "# flags below (and skips --remove/--refresh) — handy for scripts.",
            "# NOTE: [search] bottom_up is the canonical spelling; an old",
            "# [behavior] bottom_up is still accepted as a deprecated alias.",
        ],
    ),
    (
        "gui",
        &["# applies on next launch; \"device\" keeps today's behavior"],
    ),
];

/// Generate the full documented example file from the catalog (plan §7:
/// `config docs` and `--init-config` must emit identical bytes).
pub fn render_example_toml() -> String {
    let mut out = String::from(
        "# pakmap config — edit and save, changes apply on next launch\n\
         # See: https://github.com/kenjaku-dev/pakmap  — all fields optional\n",
    );
    let mut current_section = "";
    for meta in CATALOG {
        let section = section_of(meta.key);
        if section != current_section {
            if !current_section.is_empty() {
                out.push('\n');
            }
            out.push_str(&format!("\n[{section}]\n"));
            for note in SECTION_NOTES
                .iter()
                .find(|(s, _)| *s == section)
                .map(|(_, n)| *n)
                .unwrap_or(&[])
            {
                out.push_str(note);
                out.push('\n');
            }
            current_section = section;
        }
        let literal = toml_literal(meta.kind, meta.default);
        let head = format!("{} = {}", meta.key_on_file(), literal);
        out.push_str(&format!("{head:<26}# {}\n", meta.desc));
    }
    out.push_str(
        "\n# Environment variables (read-only session knobs, not file keys):\n\
         #   PAKMAP_NOCONFIRM, PAKMAP_TRUST_PROJECT, PAKMAP_SKIP_LIVE,\n\
         #   PAKMAP_GUI_SMOKE_MS, PAKMAP_GUI_FPBUDGET_MS — see `pakmap config list`.\n",
    );
    out
}

impl SettingMeta {
    /// Key as it appears inside the file: sections are headers, so the file
    /// line uses the bare key (`limit`, not `search.limit`).
    fn key_on_file(&self) -> &'static str {
        self.key.split('.').nth(1).unwrap_or(self.key)
    }
}

#[cfg(test)]
mod tests {
    //! Catalog invariants (plan §1.1/§1.2/§7). Unwraps are re-allowed for the
    //! test crate via `lib.rs`'s cfg_attr.

    use super::*;

    /// One row per canonical key: dotted keys are unique, no duplicates.
    #[test]
    fn catalog_keys_are_unique() {
        let mut keys: Vec<&str> = CATALOG.iter().map(|m| m.key).collect();
        let n = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), n);
    }

    /// §7: the generated example parses as a real `Config` and documents
    /// every catalog key exactly once, in catalog order.
    #[test]
    fn example_toml_documents_every_key_once() {
        let text = render_example_toml();
        let _parsed: crate::config::Config =
            toml::from_str(&text).expect("generated example must be valid TOML for Config");
        let got: Vec<String> = crate::settings::scanner::scan(&text)
            .keys
            .into_iter()
            .map(|f| f.key)
            .collect();
        let expected: Vec<&str> = CATALOG.iter().map(|m| m.key).collect();
        assert_eq!(
            got, expected,
            "example must list every key once, in catalog order"
        );
    }

    /// §7 / §2: `pakmap config docs` and `--init-config` emit identical
    /// bytes — both delegates to this one generator (pinned via delegation).
    #[test]
    fn docs_and_init_config_share_the_generator() {
        assert_eq!(crate::config::Config::example_toml(), render_example_toml());
    }
}
