use std::path::PathBuf;

use clap::{Parser, ValueEnum};

#[derive(Parser, Debug)]
#[command(
    name = "pakmap",
    version,
    about = "Fast search for Arch/Artix official repos + AUR",
    long_about = "Search both official pacman repositories (via libalpm) and the AUR (via RPC v5) in one command.\nExample: pakmap firefox --limit 20 --json\nTUI: pakmap --tui or pakmap (no args) launches interactive terminal (ratatui)",
    arg_required_else_help = false
)]
pub struct Cli {
    /// Search query (substring, case-insensitive). Use --regex for regex mode. If empty and --tui, opens TUI.
    pub query: Option<String>,

    /// Launch interactive TUI (search bar + results + install/info) — per tui-design full-screen session
    #[arg(long)]
    pub tui: bool,

    /// Disable TUI even when no query (plain help); also respects NO_COLOR and non-TTY
    #[arg(long)]
    pub no_tui: bool,

    /// Where to search
    #[arg(short, long, value_enum, default_value_t = Source::All, help = "Source to search")]
    pub source: Source,

    /// AUR search field (only affects AUR)
    #[arg(long, value_enum, default_value_t = AurBy::NameDesc, help = "AUR search by field")]
    pub by: AurBy,

    /// Limit results per source (0 = no limit). Repo side fills in pacman db
    /// registration order, so small limits may miss later repos.
    #[arg(short, long, default_value_t = 50)]
    pub limit: usize,

    /// Show results as JSON
    #[arg(long)]
    pub json: bool,

    /// Use regex matching (client-side filter for AUR, libalpm regex for repo)
    #[arg(short = 'x', long)]
    pub regex: bool,

    /// Show only installed packages (repo only)
    #[arg(long)]
    pub installed_only: bool,

    /// Print search results bottom-up (AUR first)
    #[arg(long)]
    pub bottom_up: bool,

    /// Verbose logs (tracing)
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Disable colored output
    #[arg(long)]
    pub no_color: bool,

    /// Path to config file (default: ~/.config/pakmap/config.toml or ./pakmap.toml).
    /// Global so it works for every `pakmap config …` subcommand too.
    #[arg(long, value_name = "PATH", global = true)]
    pub config: Option<PathBuf>,

    /// Remove an installed package and exit — non-TUI shortcut for remover mode.
    /// Auto-detects flatpak app-IDs (reverse-DNS installed via flatpak) and
    /// uninstalls without sudo; otherwise sudo pacman -R.
    /// Asks to confirm on TTY; bypass with PAKMAP_NOCONFIRM=1 (also skips makepkg/refresh prompts).
    #[arg(long, value_name = "PKG")]
    pub remove: Option<String>,

    /// Show details for one package name and exit — resolves across
    /// installed packages, sync DBs, the AUR and Flatpak (first exact-name
    /// match wins). --json emits a single JSON object; not-found exits 1.
    #[arg(long, value_name = "PKG")]
    pub info: Option<String>,

    /// Refresh pacman sync databases (sudo pacman -Sy) and exit — non-TUI shortcut.
    /// Asks to confirm on TTY; bypass with PAKMAP_NOCONFIRM=1.
    #[arg(long)]
    pub refresh: bool,

    /// List available updates (repo + AUR) and exit — non-TUI shortcut for Updates tab.
    #[arg(long)]
    pub updates: bool,

    /// List orphan packages (installed as dep, required by nothing) and exit.
    #[arg(long)]
    pub orphans: bool,

    /// Show system stats (explicit vs deps, orphans, updates, cache) and exit.
    #[arg(long)]
    pub stats: bool,

    /// Upgrade system (sudo pacman -Syu + AUR rebuild hint) — asks to confirm on TTY.
    /// Honors Arch-news guard unless PAKMAP_NOCONFIRM=1.
    #[arg(long)]
    pub upgrade: bool,

    /// Clean package cache (sudo paccache -r) — asks to confirm on TTY.
    /// Pair with --keep N (default 2) and --dry-run for a preview.
    #[arg(long)]
    pub clean: bool,

    /// Keep N versions of each package when cleaning (default 2, min 1).
    /// Only meaningful with --clean.
    #[arg(long, default_value_t = 2, value_name = "N")]
    pub keep: u32,

    /// Preview only (with --clean): show reclaimable files/bytes, remove nothing.
    #[arg(long)]
    pub dry_run: bool,

    /// Refuse all mutating ops (install/remove/refresh/upgrade) — safe browse mode.
    /// Mirrors config [behavior] readonly.
    #[arg(long)]
    pub readonly: bool,

    /// Generate example config at default location and exit
    #[arg(long)]
    pub init_config: bool,

    /// Print config path and exit
    #[arg(long)]
    pub show_config: bool,

    /// Generate a shell completion script to stdout and exit
    /// (bash | zsh | fish | powershell | nushell | elvish)
    #[arg(long, value_enum, value_name = "SHELL")]
    pub generate_completions: Option<clap_complete::Shell>,

    /// Generate the roff man page to stdout and exit (packaging helper)
    #[arg(long)]
    pub generate_man: bool,

    /// Settings operations (list/get/set/unset/validate/path/docs) — plan §2
    #[command(subcommand)]
    pub config_cmd: Option<ConfigCmd>,
}

/// clap enum for `config set/unset --target` (plan D2: user is default).
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum ConfigTarget {
    User,
    Project,
}

/// pakmap config — unified settings surface (plan §2).
#[derive(Debug, clap::Subcommand)]
pub enum ConfigCmd {
    /// List every setting (and env knobs) with value, origin, apply, danger
    List {
        /// machine-readable output
        #[arg(long)]
        json: bool,
        /// expand origins with the file path they came from
        #[arg(short, long)]
        origin: bool,
        /// filter one section family (search|tui|theme|behavior|gui|env)
        #[arg(long, value_name = "S")]
        section: Option<String>,
    },
    /// Print the effective value of one key
    Get {
        /// dotted setting key, e.g. search.limit
        key: String,
    },
    /// Write a value to the user config (default) or the project config
    Set {
        /// dotted setting key, e.g. search.limit
        key: String,
        /// value (parsed per kind: bool/number/enum/text)
        value: String,
        /// which file to write
        #[arg(
            long,
            value_enum,
            default_value_t = ConfigTarget::User,
            value_name = "user|project"
        )]
        target: ConfigTarget,
        /// create a missing user config from the documented template
        #[arg(long)]
        yes: bool,
    },
    /// Remove a key from the target file (value falls back to default)
    Unset {
        /// dotted setting key, e.g. search.limit
        key: String,
        /// which file to write
        #[arg(
            long,
            value_enum,
            default_value_t = ConfigTarget::User,
            value_name = "user|project"
        )]
        target: ConfigTarget,
    },
    /// Report unknown keys, invalid values and deprecated aliases
    Validate {
        /// machine-readable output
        #[arg(long)]
        json: bool,
        /// exit 1 when any finding exists
        #[arg(long)]
        strict: bool,
    },
    /// Print the user and project config file paths
    Path,
    /// Print the full generated example config (same bytes as --init-config)
    Docs,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum Source {
    Aur,
    Repo,
    Flatpak,
    All,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum AurBy {
    Name,
    #[value(name = "name-desc")]
    NameDesc,
    Maintainer,
    Depends,
    Makedepends,
    Optdepends,
    Checkdepends,
}

impl AurBy {
    pub fn as_str(&self) -> &'static str {
        match self {
            AurBy::Name => "name",
            AurBy::NameDesc => "name-desc",
            AurBy::Maintainer => "maintainer",
            AurBy::Depends => "depends",
            AurBy::Makedepends => "makedepends",
            AurBy::Optdepends => "optdepends",
            AurBy::Checkdepends => "checkdepends",
        }
    }
}
