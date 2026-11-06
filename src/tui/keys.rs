//! The keymap single source (overhaul phase 3, plan §3).
//!
//! Every key the TUI reacts to is declared here exactly once — key set,
//! context guard, action id — across the run loop's three structural layers:
//!
//! 1. [`Layer::Popup`] — a popup is open; it owns the keyboard (the run
//!    loop consumes *every* event while open),
//! 2. [`Layer::Modal`] — the `S` settings modal is open; it owns the
//!    keyboard (stray keys can never escape into the search),
//! 3. [`Layer::Frame`] — both closed; the app view's own bindings.
//!
//! The layer precedence itself is run-loop structure and stays frozen
//! behaviorally (D2): popup > modal > frame, evaluated in that order, each
//! layer consuming what it claims. `resolve` walks the table, first
//! key+guard match wins, rows in declaration order — which is the v0.10
//! dispatch-arm order, so walking the table restores the exact old
//! precedence. Esc unwinds exactly one layer (popup close → modal
//! sub-state → focus toggle) and `q` closes popups but quits only from
//! list focus with no popup — both now *documented and pinned* below.
//!
//! How "generated" works: the footer legend and the `?` help popup are
//! composed from the tables in this file (`FOOTER_TOKENS` + the help
//! sections consumed by `ui::chrome`/`ui::overlays`). Drift gates pin both
//! outputs byte-for-byte and cross-check them against `BINDINGS`, so a
//! keymap edit that forgets its UI copy fails a test instead of rotting the
//! legend (same pattern as the settings-doc drift gate).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::app::{Focus, Mode, Popup};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Which handler claims the press. The run loop picks the layer structurally
/// (popup open? modal open? frame) — the table rows never cross layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Popup,
    Modal,
    Frame,
}

/// One physical press to react to (the second half matches crossterm's
/// quirk: plain arms matched the *code* only, modifiers apart from the
/// checked CONTROL are never relevant).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    Key(KeyCode),
    /// Code + CONTROL held (Ctrl+R-style rows).
    Ctrl(KeyCode),
    /// Catch-all fallback row: matches any unclaimed key.
    Any,
    /// List-typing fallback: a plain character, no Ctrl/Alt.
    CharNoMods,
}

impl Press {
    fn matches(self, key: &KeyEvent) -> bool {
        match self {
            Press::Key(code) => key.code == code,
            Press::Ctrl(code) => key.code == code && key.modifiers.contains(KeyModifiers::CONTROL),
            Press::Any => true,
            Press::CharNoMods => {
                matches!(key.code, KeyCode::Char(_))
                    && !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT)
            }
        }
    }
}

/// Context predicates, one variant per guard *shape* that appears in the
/// v0.10 arms. Frame guards assume the layer invariant (no popup / no modal)
/// — the run loop guarantees it, so the old arms' redundant
/// `popup == Popup::None` clauses are wording, not behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guard {
    /// Guard shape for the old arms with *no* guard at all (Tab/F5/Ctrl
    /// cycles — fires anywhere in the frame).
    Any,
    // --- frame ---
    Search,
    List,
    /// List focus scoped to specific modes (`o`, `c`, `U`, `d`… rows).
    ListIn(ModeMask),
    // --- modal (settings open) ---
    /// The value editor box is open.
    EditorOn,
    /// The filter box is focused.
    FilterOn,
    /// Neither editor nor filter: the modal's list level.
    Plain,
    /// "Not editing" — the modal guard exactly as v0.10 had it (`Up`/`Down`
    /// also still work while the filter is focused).
    NotEditing,
    // --- popup ---
    PopupConfirm,
    /// Any popup that Enter closes (info/message/help); v0.10 included Help
    /// in its catch-all, so Help counts as Enter-closable here.
    PopupClosable,
    /// The help popup itself (the `?` toggle's exact old guard).
    PopupHelp,
    PopupAny,
}

impl Guard {
    fn check(self, ctx: Ctx) -> bool {
        match self {
            Guard::Any => true,
            Guard::Search => ctx.focus == Focus::Search,
            Guard::List => ctx.focus == Focus::List,
            Guard::ListIn(mask) => ctx.focus == Focus::List && mask.contains(ctx.mode),
            Guard::EditorOn => ctx.modal == Some(ModalState::Editing),
            Guard::FilterOn => ctx.modal == Some(ModalState::Filtering),
            Guard::Plain => ctx.modal == Some(ModalState::Plain),
            Guard::NotEditing => ctx.modal.is_some_and(|m| m != ModalState::Editing),
            Guard::PopupConfirm => ctx.popup == Some(PopupClass::Confirm),
            Guard::PopupClosable => ctx.popup.is_some_and(|c| c != PopupClass::Confirm),
            Guard::PopupHelp => ctx.popup == Some(PopupClass::Help),
            Guard::PopupAny => ctx.popup.is_some(),
        }
    }
}

/// Mode scope bitset for the mode-scoped list rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeMask(u8);

impl ModeMask {
    pub const SEARCH: Self = Self(1);
    pub const INSTALLED: Self = Self(2);
    pub const UPDATES: Self = Self(4);
    pub const INSTALLED_UPDATES: Self = Self(6);

    pub const fn contains(self, mode: Mode) -> bool {
        let bit = match mode {
            Mode::Search => Self::SEARCH.0,
            Mode::Installed => Self::INSTALLED.0,
            Mode::Updates => Self::UPDATES.0,
        };
        self.0 & bit != 0
    }
}

/// The modal's own third axis of guards (its list level + the two capture
/// boxes — exactly the `editing`/`filtering` pair the handler computed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalState {
    Editing,
    Filtering,
    Plain,
}

/// Popup classification used by the popup rows (the two confirm flavors are
/// behaviorally identical; info/message are Enter-closable; help is the
/// one popup `?` toggles).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupClass {
    Confirm,
    Closable,
    Help,
}

impl PopupClass {
    pub fn of(popup: &Popup) -> Self {
        match popup {
            Popup::Confirm(_)
            | Popup::ConfirmRemove(_)
            | Popup::ConfirmRefresh
            | Popup::ConfirmUpgrade
            | Popup::ConfirmClean { .. } => Self::Confirm,
            Popup::Help => Self::Help,
            _ => Self::Closable,
        }
    }
}

/// The context a layer resolves against — built once per event by the
/// handler that owns the layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ctx {
    pub mode: Mode,
    pub focus: Focus,
    pub popup: Option<PopupClass>,
    pub modal: Option<ModalState>,
}

impl Ctx {
    pub fn frame(mode: Mode, focus: Focus) -> Self {
        Self {
            mode,
            focus,
            popup: None,
            modal: None,
        }
    }

    pub fn modal(editing: bool, filtering: bool) -> Self {
        let state = if editing {
            ModalState::Editing
        } else if filtering {
            ModalState::Filtering
        } else {
            ModalState::Plain
        };
        Self {
            mode: Mode::Search,
            focus: Focus::List,
            popup: None,
            modal: Some(state),
        }
    }

    pub fn popup(class: PopupClass) -> Self {
        Self {
            mode: Mode::Search,
            focus: Focus::List,
            popup: Some(class),
            modal: None,
        }
    }
}

/// What to do. One variant per v0.10 arm (compound arms split per focus /
/// modal sub-state so each row's guard is exact); bodies live in the
/// `execute` match in `app.rs`, moved verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    // --- popup layer ---
    /// Esc/q on a popup: close it and drop any upgrade-warning copy.
    ClosePopupDismiss,
    /// `?` toggles the help popup closed (only when it is already open).
    ToggleHelpPopup,
    /// Enter/Y on a confirm dialog: run the dialog's action.
    ConfirmActive,
    /// Enter on info/message/help or `n` on anything: plain close.
    ClosePopupSimple,
    // --- modal layer ---
    ModalCancelEditor,
    ModalClearFilter,
    /// Esc at list level (discards drafts) and `S` (toggles closed) share it.
    ModalClose,
    ModalMovePrev,
    ModalMoveNext,
    ModalPrevGroup,
    ModalNextGroup,
    ModalFilterFocus,
    ModalCommitEditor,
    ModalLeaveFilter,
    ModalEnterRow,
    ModalToggleBool,
    ModalResetRow,
    ModalSave,
    ModalEditFile,
    /// The modal capture boxes swallow everything else while open (typing
    /// guard — stray keys can never escape into the search).
    ModalTypeEditor,
    ModalTypeFilter,
    // --- frame layer ---
    CycleModeNext,
    CycleModePrev,
    OpenPopupHelp,
    OpenSettings,
    ConfirmRefresh,
    /// `q` in list focus **and** Ctrl+C anywhere — quit. Inability that
    /// stays: `q` while typing types; `q` inside popups closes them.
    Quit,
    /// Esc in search focus: back to the list (one unwind layer).
    ToggleToList,
    /// Esc in list focus: to the search box (one unwind layer).
    ToggleToSearch,
    /// `/` jumps to the search box from list focus; while already focused
    /// the old arm re-set the focus (harmless no-op kept verbatim).
    JumpToSearch,
    SubmitSearch,
    /// Enter on a list row: mode-dependent confirm dialog.
    ConfirmSelected,
    UpgradeAll,
    ToggleOrphans,
    CleanPreview,
    RemoveSelected,
    InfoSelected,
    MovePrev,
    MoveNext,
    /// The `i`/`U??` — no: the search branch of the old `i`, `Up/Down` arms,
    /// which set the debounce flags unconditionally (kept verbatim).
    TypeForced,
    /// The catch-all typing discipline: forward to the input, flag only on
    /// change (the old `_` arm).
    TypeSearch,
    /// List focus: an unclaimed plain character jumps to search and types.
    TypeFromList,
}

/// One declared binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub layer: Layer,
    /// All key aliases in one row (`[[Ctrl(Key)]]`-style sets, exactly the
    /// old arm's `|` alternatives).
    pub keys: &'static [Press],
    pub guard: Guard,
    pub action: Action,
    /// Binding deliberately absent from the legends (doc `hidden`): rows
    /// where the old UI never showed the keys, or structural fallbacks.
    pub hidden: bool,
    /// Press labels missing from the legend *copy* without the row being
    /// hidden (arrows shown as "↑↓", uppercase aliases folded into the
    /// lowercased keycap, modal arrows shown as "←→"). The coverage gate
    /// skips exactly these labels.
    pub doc_skips: &'static [&'static str],
}

// ---------------------------------------------------------------------------
// The table — row order *is* the v0.10 dispatch-arm order per layer
// ---------------------------------------------------------------------------

static ESC: Press = Press::Key(KeyCode::Esc);

/// Every binding in the TUI. Position within a layer == the old match-arm
/// position; `resolve` walks first-match-wins, so the run loop's behavior is
/// the table verbatim.
pub static BINDINGS: &[Binding] = &[
    // ---- popup layer (a popup is open; everything is consumed) ----
    Binding {
        layer: Layer::Popup,
        keys: &[ESC, Press::Key(KeyCode::Char('q'))],
        guard: Guard::PopupAny,
        action: Action::ClosePopupDismiss,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Popup,
        keys: &[Press::Key(KeyCode::Char('?'))],
        guard: Guard::PopupHelp,
        action: Action::ToggleHelpPopup,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Popup,
        keys: &[Press::Key(KeyCode::Enter)],
        guard: Guard::PopupConfirm,
        action: Action::ConfirmActive,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Popup,
        keys: &[Press::Key(KeyCode::Enter)],
        guard: Guard::PopupClosable,
        action: Action::ClosePopupSimple,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Popup,
        keys: &[
            Press::Key(KeyCode::Char('y')),
            Press::Key(KeyCode::Char('Y')),
        ],
        guard: Guard::PopupConfirm,
        action: Action::ConfirmActive,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Popup,
        keys: &[
            Press::Key(KeyCode::Char('n')),
            Press::Key(KeyCode::Char('N')),
        ],
        guard: Guard::PopupAny,
        action: Action::ClosePopupSimple,
        hidden: false,
        doc_skips: &[],
    },
    // ---- modal layer (settings modal open; the typing guard swallows) ----
    Binding {
        layer: Layer::Modal,
        keys: &[ESC],
        guard: Guard::EditorOn,
        action: Action::ModalCancelEditor,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Modal,
        keys: &[ESC],
        guard: Guard::FilterOn,
        action: Action::ModalClearFilter,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Modal,
        keys: &[ESC],
        guard: Guard::Plain,
        action: Action::ModalClose,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Modal,
        keys: &[Press::Key(KeyCode::Char('S'))],
        guard: Guard::Plain,
        action: Action::ModalClose,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Modal,
        keys: &[Press::Key(KeyCode::Up), Press::Key(KeyCode::Char('k'))],
        guard: Guard::NotEditing,
        action: Action::ModalMovePrev,
        hidden: false,
        doc_skips: &["Up"],
    },
    Binding {
        layer: Layer::Modal,
        keys: &[Press::Key(KeyCode::Down), Press::Key(KeyCode::Char('j'))],
        guard: Guard::NotEditing,
        action: Action::ModalMoveNext,
        hidden: false,
        doc_skips: &["Down"],
    },
    Binding {
        layer: Layer::Modal,
        keys: &[Press::Key(KeyCode::Left), Press::Key(KeyCode::Char('h'))],
        guard: Guard::Plain,
        action: Action::ModalPrevGroup,
        hidden: false,
        doc_skips: &["Left", "h"],
    },
    Binding {
        layer: Layer::Modal,
        keys: &[Press::Key(KeyCode::Right), Press::Key(KeyCode::Char('l'))],
        guard: Guard::Plain,
        action: Action::ModalNextGroup,
        hidden: false,
        doc_skips: &["Right", "l"],
    },
    Binding {
        layer: Layer::Modal,
        keys: &[Press::Key(KeyCode::Char('/'))],
        guard: Guard::NotEditing,
        action: Action::ModalFilterFocus,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Modal,
        keys: &[Press::Key(KeyCode::Enter)],
        guard: Guard::EditorOn,
        action: Action::ModalCommitEditor,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Modal,
        keys: &[Press::Key(KeyCode::Enter)],
        guard: Guard::FilterOn,
        action: Action::ModalLeaveFilter,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Modal,
        keys: &[Press::Key(KeyCode::Enter)],
        guard: Guard::Plain,
        action: Action::ModalEnterRow,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Modal,
        keys: &[Press::Key(KeyCode::Char(' '))],
        guard: Guard::Plain,
        action: Action::ModalToggleBool,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Modal,
        keys: &[Press::Key(KeyCode::Char('r'))],
        guard: Guard::Plain,
        action: Action::ModalResetRow,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Modal,
        keys: &[Press::Key(KeyCode::Char('s'))],
        guard: Guard::Plain,
        action: Action::ModalSave,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Modal,
        keys: &[Press::Key(KeyCode::Char('e'))],
        guard: Guard::Plain,
        action: Action::ModalEditFile,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Modal,
        keys: &[Press::Any],
        guard: Guard::EditorOn,
        action: Action::ModalTypeEditor,
        hidden: true,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Modal,
        keys: &[Press::Any],
        guard: Guard::FilterOn,
        action: Action::ModalTypeFilter,
        hidden: true,
        doc_skips: &[],
    },
    // ---- frame layer (no popup, no modal) — the v0.10 arm order ----
    Binding {
        layer: Layer::Frame,
        keys: &[Press::Key(KeyCode::Tab)],
        guard: Guard::Any,
        action: Action::CycleModeNext,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Frame,
        keys: &[Press::Key(KeyCode::BackTab)],
        guard: Guard::Any,
        action: Action::CycleModePrev,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Frame,
        keys: &[Press::Key(KeyCode::Char('?'))],
        guard: Guard::List,
        action: Action::OpenPopupHelp,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Frame,
        keys: &[Press::Key(KeyCode::Char('S'))],
        guard: Guard::List,
        action: Action::OpenSettings,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Frame,
        keys: &[
            Press::Key(KeyCode::Char('r')),
            Press::Key(KeyCode::Char('R')),
        ],
        guard: Guard::List,
        action: Action::ConfirmRefresh,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Frame,
        keys: &[Press::Key(KeyCode::F(5))],
        guard: Guard::Any,
        action: Action::ConfirmRefresh,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Frame,
        keys: &[
            Press::Ctrl(KeyCode::Char('r')),
            Press::Ctrl(KeyCode::Char('R')),
        ],
        guard: Guard::Any,
        action: Action::ConfirmRefresh,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Frame,
        keys: &[Press::Key(KeyCode::Char('q'))],
        guard: Guard::List,
        action: Action::Quit,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Frame,
        keys: &[Press::Ctrl(KeyCode::Char('c'))],
        guard: Guard::Any,
        action: Action::Quit,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Frame,
        keys: &[ESC],
        guard: Guard::Search,
        action: Action::ToggleToList,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Frame,
        keys: &[ESC],
        guard: Guard::List,
        action: Action::ToggleToSearch,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Frame,
        keys: &[Press::Key(KeyCode::Char('/'))],
        guard: Guard::Any,
        action: Action::JumpToSearch,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Frame,
        keys: &[Press::Key(KeyCode::Enter)],
        guard: Guard::Search,
        action: Action::SubmitSearch,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Frame,
        keys: &[Press::Key(KeyCode::Enter)],
        guard: Guard::List,
        action: Action::ConfirmSelected,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Frame,
        keys: &[Press::Key(KeyCode::Char('U'))],
        guard: Guard::ListIn(ModeMask::UPDATES),
        action: Action::UpgradeAll,
        hidden: false,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Frame,
        keys: &[
            Press::Key(KeyCode::Char('o')),
            Press::Key(KeyCode::Char('O')),
        ],
        guard: Guard::ListIn(ModeMask::INSTALLED),
        action: Action::ToggleOrphans,
        hidden: false,
        doc_skips: &["O"],
    },
    Binding {
        layer: Layer::Frame,
        keys: &[
            Press::Key(KeyCode::Char('c')),
            Press::Key(KeyCode::Char('C')),
        ],
        guard: Guard::ListIn(ModeMask::INSTALLED_UPDATES),
        action: Action::CleanPreview,
        hidden: false,
        doc_skips: &[],
    },
    // doc-hidden: the old UI never advertised Delete/Backspace in the
    // legend/help; the d/D/x/X row below covers remove.
    Binding {
        layer: Layer::Frame,
        keys: &[Press::Key(KeyCode::Delete), Press::Key(KeyCode::Backspace)],
        guard: Guard::ListIn(ModeMask::INSTALLED),
        action: Action::RemoveSelected,
        hidden: true,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Frame,
        keys: &[
            Press::Key(KeyCode::Char('d')),
            Press::Key(KeyCode::Char('D')),
            Press::Key(KeyCode::Char('x')),
            Press::Key(KeyCode::Char('X')),
        ],
        guard: Guard::ListIn(ModeMask::INSTALLED),
        action: Action::RemoveSelected,
        hidden: false,
        doc_skips: &["D", "X"],
    },
    Binding {
        layer: Layer::Frame,
        keys: &[
            Press::Key(KeyCode::Char('i')),
            Press::Key(KeyCode::Char('I')),
        ],
        guard: Guard::List,
        action: Action::InfoSelected,
        hidden: false,
        doc_skips: &["I"],
    },
    // The old `i` arm's Search branch: unconditional debounce flags.
    Binding {
        layer: Layer::Frame,
        keys: &[
            Press::Key(KeyCode::Char('i')),
            Press::Key(KeyCode::Char('I')),
        ],
        guard: Guard::Search,
        action: Action::TypeForced,
        hidden: false,
        doc_skips: &["I"],
    },
    Binding {
        layer: Layer::Frame,
        keys: &[Press::Key(KeyCode::Up), Press::Key(KeyCode::Char('k'))],
        guard: Guard::List,
        action: Action::MovePrev,
        hidden: false,
        doc_skips: &["Up"],
    },
    // Old Up/`k` arm's Search branch (unconditional flags).
    Binding {
        layer: Layer::Frame,
        keys: &[Press::Key(KeyCode::Up), Press::Key(KeyCode::Char('k'))],
        guard: Guard::Search,
        action: Action::TypeForced,
        hidden: false,
        doc_skips: &["Up", "k"],
    },
    Binding {
        layer: Layer::Frame,
        keys: &[Press::Key(KeyCode::Down), Press::Key(KeyCode::Char('j'))],
        guard: Guard::List,
        action: Action::MoveNext,
        hidden: false,
        doc_skips: &["Down"],
    },
    // Old Down/`j` arm's Search branch.
    Binding {
        layer: Layer::Frame,
        keys: &[Press::Key(KeyCode::Down), Press::Key(KeyCode::Char('j'))],
        guard: Guard::Search,
        action: Action::TypeForced,
        hidden: false,
        doc_skips: &["Down", "j"],
    },
    Binding {
        layer: Layer::Frame,
        keys: &[Press::Any],
        guard: Guard::Search,
        action: Action::TypeSearch,
        hidden: true,
        doc_skips: &[],
    },
    Binding {
        layer: Layer::Frame,
        keys: &[Press::CharNoMods],
        guard: Guard::List,
        action: Action::TypeFromList,
        hidden: true,
        doc_skips: &[],
    },
];

/// Resolve one press through one layer: first binding (declaration order ==
/// v0.10 arm order) whose keys match *and* whose guard holds. `None` means
/// "claimed silently" everywhere the old code fell into an empty `_ => {}`
/// body — no action, still consumed by the layer.
pub fn resolve(layer: Layer, ctx: Ctx, key: &KeyEvent) -> Option<Action> {
    BINDINGS
        .iter()
        .filter(|b| b.layer == layer && b.guard.check(ctx))
        .find(|b| b.keys.iter().any(|p| p.matches(key)))
        .map(|b| b.action)
}

// ---------------------------------------------------------------------------
// Generated legends (the footer + the `?` popup)
// ---------------------------------------------------------------------------

/// One footer legend token, tagged with the action it advertises and the
/// (mode, focus) pair it is displayed for. Array order == display order per
/// pair; the generator filters, so each pair's slots come out byte-exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot {
    mode: Mode,
    focus: Focus,
    action: Action,
    token: &'static str,
}

use Focus::{List as L, Search as S};
use Mode::{Installed as I, Search as Sm, Updates as U};

/// The complete footer legend copy — generated into
/// `ui::chrome::draw_help` (one leading space, single-space joins, one
/// trailing space — the v0.10 bytes).
static FOOTER_TOKENS: &[Slot] = &[
    // --- search focus ---
    Slot {
        mode: Sm,
        focus: S,
        action: Action::SubmitSearch,
        token: "Enter:search",
    },
    Slot {
        mode: I,
        focus: S,
        action: Action::SubmitSearch,
        token: "Enter:filter",
    },
    Slot {
        mode: U,
        focus: S,
        action: Action::SubmitSearch,
        token: "Enter:filter",
    },
    Slot {
        mode: I,
        focus: S,
        action: Action::ToggleOrphans,
        token: "o:orphans",
    },
    Slot {
        mode: U,
        focus: S,
        action: Action::UpgradeAll,
        token: "U:upgrade-all",
    },
    Slot {
        mode: Sm,
        focus: S,
        action: Action::ConfirmRefresh,
        token: "F5:refresh",
    },
    Slot {
        mode: I,
        focus: S,
        action: Action::ConfirmRefresh,
        token: "F5:refresh",
    },
    Slot {
        mode: U,
        focus: S,
        action: Action::ConfirmRefresh,
        token: "F5:refresh",
    },
    Slot {
        mode: Sm,
        focus: S,
        action: Action::ToggleToList,
        token: "Esc:list",
    },
    Slot {
        mode: I,
        focus: S,
        action: Action::ToggleToList,
        token: "Esc:list",
    },
    Slot {
        mode: U,
        focus: S,
        action: Action::ToggleToList,
        token: "Esc:list",
    },
    Slot {
        mode: Sm,
        focus: S,
        action: Action::CycleModeNext,
        token: "Tab:remover",
    },
    Slot {
        mode: I,
        focus: S,
        action: Action::CycleModeNext,
        token: "Tab:updates",
    },
    Slot {
        mode: U,
        focus: S,
        action: Action::CycleModeNext,
        token: "Tab:search",
    },
    Slot {
        mode: Sm,
        focus: S,
        action: Action::OpenPopupHelp,
        token: "?:help",
    },
    Slot {
        mode: I,
        focus: S,
        action: Action::OpenPopupHelp,
        token: "?:help",
    },
    Slot {
        mode: U,
        focus: S,
        action: Action::OpenPopupHelp,
        token: "?:help",
    },
    Slot {
        mode: Sm,
        focus: S,
        action: Action::Quit,
        token: "Ctrl+C:quit",
    },
    Slot {
        mode: I,
        focus: S,
        action: Action::Quit,
        token: "Ctrl+C:quit",
    },
    Slot {
        mode: U,
        focus: S,
        action: Action::Quit,
        token: "Ctrl+C:quit",
    },
    // --- list focus ---
    Slot {
        mode: Sm,
        focus: L,
        action: Action::MovePrev,
        token: "↑↓/j k:move",
    },
    Slot {
        mode: I,
        focus: L,
        action: Action::MovePrev,
        token: "↑↓/j k:move",
    },
    Slot {
        mode: U,
        focus: L,
        action: Action::MovePrev,
        token: "↑↓/j k:move",
    },
    Slot {
        mode: I,
        focus: L,
        action: Action::ConfirmSelected,
        token: "Enter/d:remove",
    },
    Slot {
        mode: Sm,
        focus: L,
        action: Action::ConfirmSelected,
        token: "Enter:install",
    },
    Slot {
        mode: U,
        focus: L,
        action: Action::ConfirmSelected,
        token: "Enter:upgrade-one",
    },
    Slot {
        mode: I,
        focus: L,
        action: Action::ToggleOrphans,
        token: "o:orphans",
    },
    Slot {
        mode: U,
        focus: L,
        action: Action::UpgradeAll,
        token: "U:upgrade-all",
    },
    Slot {
        mode: I,
        focus: L,
        action: Action::CleanPreview,
        token: "c:clean",
    },
    Slot {
        mode: U,
        focus: L,
        action: Action::CleanPreview,
        token: "c:clean",
    },
    Slot {
        mode: Sm,
        focus: L,
        action: Action::InfoSelected,
        token: "i:info",
    },
    Slot {
        mode: I,
        focus: L,
        action: Action::InfoSelected,
        token: "i:info",
    },
    Slot {
        mode: U,
        focus: L,
        action: Action::InfoSelected,
        token: "i:info",
    },
    Slot {
        mode: Sm,
        focus: L,
        action: Action::ConfirmRefresh,
        token: "r/F5:refresh",
    },
    Slot {
        mode: Sm,
        focus: L,
        action: Action::OpenSettings,
        token: "S:settings",
    },
    Slot {
        mode: I,
        focus: L,
        action: Action::OpenSettings,
        token: "S:settings",
    },
    Slot {
        mode: U,
        focus: L,
        action: Action::OpenSettings,
        token: "S:settings",
    },
    Slot {
        mode: Sm,
        focus: L,
        action: Action::CycleModeNext,
        token: "Tab:remover",
    },
    Slot {
        mode: I,
        focus: L,
        action: Action::CycleModeNext,
        token: "Tab:updates",
    },
    Slot {
        mode: U,
        focus: L,
        action: Action::CycleModeNext,
        token: "Tab:search",
    },
    Slot {
        mode: Sm,
        focus: L,
        action: Action::OpenPopupHelp,
        token: "?:help",
    },
    Slot {
        mode: I,
        focus: L,
        action: Action::OpenPopupHelp,
        token: "?:help",
    },
    Slot {
        mode: U,
        focus: L,
        action: Action::OpenPopupHelp,
        token: "?:help",
    },
    Slot {
        mode: Sm,
        focus: L,
        action: Action::Quit,
        token: "q:quit",
    },
    Slot {
        mode: I,
        focus: L,
        action: Action::Quit,
        token: "q:quit",
    },
    Slot {
        mode: U,
        focus: L,
        action: Action::Quit,
        token: "q:quit",
    },
];

/// The generated footer legend for `(mode, focus)`: one space-padded line,
/// tokens in table order — the v0.10 strings, byte-for-byte.
pub fn footer_legend(mode: Mode, focus: Focus) -> String {
    let mut out = String::from(" ");
    let mut first = true;
    for slot in FOOTER_TOKENS {
        if slot.mode == mode && slot.focus == focus {
            if !first {
                out.push(' ');
            }
            out.push_str(slot.token);
            first = false;
        }
    }
    out.push(' ');
    out
}

/// Help-popup sections in exact display order (the Settings section is the
/// generated modal keymap description; the closing line documents the popup
/// layer's unwinds). Blanks are interpolated by `help_lines` at the pinned
/// positions.
static HELP_SECTIONS: &[&[&str]] = &[
    &[
        "Tab / BackTab : cycle Search -> Installed -> Updates",
        "F5 / Ctrl+R   : refresh sync DBs (confirm, anywhere)",
    ],
    &[
        "Search    : type+Enter search, ↑↓ move, Enter install,",
        "              i info+deps, r refresh (list)",
    ],
    &[
        "Installed : filter, Enter/d/x remove, o orphans-only,",
        "              c clean cache, i info, r refresh (list)",
    ],
    &[
        "Updates   : filter, Enter upgrade-one, U upgrade-all,",
        "              c clean cache, i info",
    ],
    &["Global    : / search, Esc toggle, ? help, q quit (list)"],
    &[
        "Settings  : S opens the settings modal (list focus) — every",
        "            config key: / filter, Space toggle, Enter edit,",
        "            r reset, s save (user file), e $EDITOR, ←→ categories",
    ],
    &["Press Esc/q/Enter/? to close"],
];

/// The generated `?` popup content: sections flattened with the pinned
/// blank separators (after the global-keys section and before the close
/// note). Consumed by `ui::overlays::draw_help_popup`.
pub fn help_lines() -> Vec<&'static str> {
    let mut lines = Vec::new();
    let last = HELP_SECTIONS.len() - 1;
    for (i, section) in HELP_SECTIONS.iter().enumerate() {
        if i == 1 || i == last {
            lines.push("");
        }
        lines.extend_from_slice(section);
    }
    lines
}

/// Phase 4: the debug-timing variant — `note` (Some only under
/// `PAKMAP_TUI_DEBUG`) appends one blank separator + the instrumentation
/// line after the close note. The default stays the pinned `help_lines`
/// byte-for-byte; the drift gate ties to `Help` strings, and this variant
/// keeps its own tacked-on line test-visible.
pub fn help_lines_with(note: Option<&str>) -> Vec<String> {
    let mut lines: Vec<String> = help_lines().into_iter().map(str::to_string).collect();
    if let Some(note) = note {
        lines.push(String::new());
        lines.push(format!("Debug: {note}"));
    }
    lines
}

// ---------------------------------------------------------------------------
// Drift gates (plan §3: help text pinned to the keymap, not hand-kept)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Press -> legend-space display name (what the gates match against).
    fn press_label(p: Press) -> String {
        match p {
            Press::Key(code) => match code {
                KeyCode::Esc => "Esc".into(),
                KeyCode::Enter => "Enter".into(),
                KeyCode::Tab => "Tab".into(),
                KeyCode::BackTab => "BackTab".into(),
                KeyCode::Up => "Up".into(),
                KeyCode::Down => "Down".into(),
                KeyCode::Left => "Left".into(),
                KeyCode::Right => "Right".into(),
                KeyCode::Delete => "Delete".into(),
                KeyCode::Backspace => "Backspace".into(),
                KeyCode::F(5) => "F5".into(),
                KeyCode::Char(c) => c.to_string(),
                other => format!("{other:?}"),
            },
            Press::Ctrl(code) => match code {
                KeyCode::Char(c) => format!("Ctrl+{}", c.to_ascii_uppercase()),
                other => format!("{other:?}"),
            },
            Press::Any => "any key".into(),
            Press::CharNoMods => "letter".into(),
        }
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    const ALL_MODES: [Mode; 3] = [Mode::Search, Mode::Installed, Mode::Updates];
    const BOTH_FOCUS: [Focus; 2] = [Focus::Search, Focus::List];

    /// Acceptance (plan §3): the footer legend is *generated* — all six
    /// v0.10 strings pinned byte-exact so the generation is proven driftless
    /// against the shipped copy.
    #[test]
    fn footer_legend_is_generated_and_matches_v010() {
        let expected = |m: Mode, f: Focus| match (m, f) {
            (Mode::Search, Focus::Search) => {
                " Enter:search F5:refresh Esc:list Tab:remover ?:help Ctrl+C:quit "
            }
            (Mode::Search, Focus::List) => {
                " ↑↓/j k:move Enter:install i:info r/F5:refresh S:settings Tab:remover ?:help q:quit "
            }
            (Mode::Installed, Focus::Search) => {
                " Enter:filter o:orphans F5:refresh Esc:list Tab:updates ?:help Ctrl+C:quit "
            }
            (Mode::Installed, Focus::List) => {
                " ↑↓/j k:move Enter/d:remove o:orphans c:clean i:info S:settings Tab:updates ?:help q:quit "
            }
            (Mode::Updates, Focus::Search) => {
                " Enter:filter U:upgrade-all F5:refresh Esc:list Tab:search ?:help Ctrl+C:quit "
            }
            (Mode::Updates, Focus::List) => {
                " ↑↓/j k:move Enter:upgrade-one U:upgrade-all c:clean i:info S:settings Tab:search ?:help q:quit "
            }
        };
        for m in ALL_MODES {
            for f in BOTH_FOCUS {
                assert_eq!(
                    footer_legend(m, f),
                    expected(m, f),
                    "footer legend drifted for ({m:?}, {f:?})"
                );
            }
        }
    }

    /// The `?` popup content is generated, section order + blanks pinned to
    /// the v0.10 bytes.
    #[test]
    fn help_lines_match_v010() {
        assert_eq!(
            help_lines(),
            vec![
                "Tab / BackTab : cycle Search -> Installed -> Updates",
                "F5 / Ctrl+R   : refresh sync DBs (confirm, anywhere)",
                "",
                "Search    : type+Enter search, ↑↓ move, Enter install,",
                "              i info+deps, r refresh (list)",
                "Installed : filter, Enter/d/x remove, o orphans-only,",
                "              c clean cache, i info, r refresh (list)",
                "Updates   : filter, Enter upgrade-one, U upgrade-all,",
                "              c clean cache, i info",
                "Global    : / search, Esc toggle, ? help, q quit (list)",
                "Settings  : S opens the settings modal (list focus) — every",
                "            config key: / filter, Space toggle, Enter edit,",
                "            r reset, s save (user file), e $EDITOR, ←→ categories",
                "",
                "Press Esc/q/Enter/? to close",
            ]
        );
    }

    /// Phase 4: the debug variant tucks exactly one blank separator + one
    /// `Debug: …` line after the pinned content, and stays byte-identical
    /// to the default when no note is set (the shipped popup can never
    /// change shape from the instrumentation).
    #[test]
    fn help_lines_with_appends_debug_note_only() {
        let mut base: Vec<String> = help_lines().iter().map(|s| s.to_string()).collect();
        assert_eq!(help_lines_with(None), base, "None variant == default bytes");
        let mut with = help_lines_with(Some("last search observed at 132 ms"));
        assert_eq!(
            with.pop(),
            Some("Debug: last search observed at 132 ms".into())
        );
        assert_eq!(with.pop(), Some(String::new()));
        while with.len() > base.len() {
            assert_eq!(with.pop(), base.pop());
        }
        assert_eq!(with, base, "prefix stays pinned");
    }

    /// Every legend token advertises a REAL binding: the advertised action
    /// exists in the frame table, and every named keycap piece resolves to
    /// some frame press (arrow glyphs like `↑↓` are notation-only).
    #[test]
    fn legend_tokens_reference_real_bindings() {
        let press_haystack: String = BINDINGS
            .iter()
            .filter(|b| b.layer == Layer::Frame)
            .flat_map(|b| b.keys.iter().map(|p| press_label(*p)))
            .collect();
        for slot in FOOTER_TOKENS {
            assert!(
                BINDINGS
                    .iter()
                    .any(|b| b.layer == Layer::Frame && b.action == slot.action),
                "footer token {slot:?} advertises an action with no frame binding"
            );
            for piece in slot
                .token
                .split(':')
                .next()
                .unwrap_or(slot.token)
                .split(['/', ' '])
            {
                if piece.is_empty() || piece == "↑↓" || piece == "←→" {
                    continue;
                }
                assert!(
                    press_haystack
                        .to_ascii_lowercase()
                        .contains(piece.to_ascii_lowercase().as_str()),
                    "footer token `{}` references key piece `{piece}` with no frame binding press",
                    slot.token
                );
            }
        }
    }

    /// Drift gate: every non-hidden binding displays somewhere in the
    /// generated legends (footer tokens + help lines), modulo its explicit
    /// `doc_skips`. A keymap change that forgets its copy fails here.
    #[test]
    fn every_visible_binding_is_documented_in_the_legends() {
        let mut haystack: String = FOOTER_TOKENS.iter().map(|s| s.token).collect();
        for section in HELP_SECTIONS {
            for line in *section {
                haystack.push_str(line);
            }
        }
        for b in BINDINGS {
            if b.hidden {
                continue;
            }
            for p in b.keys {
                let label = press_label(*p);
                if b.doc_skips.contains(&label.as_str()) {
                    continue;
                }
                assert!(
                    haystack
                        .to_ascii_lowercase()
                        .contains(label.to_ascii_lowercase().as_str()),
                    "binding {b:?} press `{label}` is not documented in footer/help copy"
                );
            }
        }
    }

    /// The dispatch matrix at the pure level: state × key → action, mirroring
    /// the v0.10 arm order. (The applied-level matrix, driving a real App
    /// through the run-loop layers, lives in `app.rs` tests.)
    #[test]
    fn resolve_frame_matrix_matches_the_v010_arms() {
        fn f(m: Mode, fo: Focus, ev: &KeyEvent) -> Option<Action> {
            resolve(Layer::Frame, Ctx::frame(m, fo), ev)
        }
        for m in ALL_MODES {
            // Tab/BackTab cycle anywhere; Esc toggles; '/' jumps.
            assert_eq!(
                f(m, Focus::Search, &press(KeyCode::Tab)),
                Some(Action::CycleModeNext)
            );
            assert_eq!(
                f(m, Focus::List, &press(KeyCode::Tab)),
                Some(Action::CycleModeNext)
            );
            assert_eq!(
                f(m, Focus::List, &press(KeyCode::BackTab)),
                Some(Action::CycleModePrev)
            );
            assert_eq!(
                f(m, Focus::Search, &press(KeyCode::Esc)),
                Some(Action::ToggleToList)
            );
            assert_eq!(
                f(m, Focus::List, &press(KeyCode::Esc)),
                Some(Action::ToggleToSearch)
            );
            assert_eq!(
                f(m, Focus::List, &press(KeyCode::Char('/'))),
                Some(Action::JumpToSearch)
            );
            assert_eq!(
                f(m, Focus::Search, &press(KeyCode::Char('/'))),
                Some(Action::JumpToSearch)
            );
            // F5 and Ctrl(+r|R) refresh from anywhere.
            assert_eq!(
                f(m, Focus::Search, &press(KeyCode::F(5))),
                Some(Action::ConfirmRefresh)
            );
            assert_eq!(
                f(m, Focus::List, &press(KeyCode::F(5))),
                Some(Action::ConfirmRefresh)
            );
            assert_eq!(
                f(m, Focus::Search, &ctrl(KeyCode::Char('r'))),
                Some(Action::ConfirmRefresh)
            );
            assert_eq!(
                f(m, Focus::List, &ctrl(KeyCode::Char('R'))),
                Some(Action::ConfirmRefresh)
            );
            // Ctrl+C quits anywhere; ? and S and plain q own List focus only.
            assert_eq!(
                f(m, Focus::List, &ctrl(KeyCode::Char('c'))),
                Some(Action::Quit)
            );
            assert_eq!(
                f(m, Focus::Search, &ctrl(KeyCode::Char('c'))),
                Some(Action::Quit)
            );
            assert_eq!(
                f(m, Focus::List, &press(KeyCode::Char('?'))),
                Some(Action::OpenPopupHelp)
            );
            assert_eq!(
                f(m, Focus::Search, &press(KeyCode::Char('?'))),
                Some(Action::TypeSearch)
            );
            assert_eq!(
                f(m, Focus::List, &press(KeyCode::Char('S'))),
                Some(Action::OpenSettings)
            );
            assert_eq!(
                f(m, Focus::Search, &press(KeyCode::Char('S'))),
                Some(Action::TypeSearch)
            );
            assert_eq!(
                f(m, Focus::List, &press(KeyCode::Char('q'))),
                Some(Action::Quit)
            );
            // `q` while typing types — never quits (documented semantics).
            assert_eq!(
                f(m, Focus::Search, &press(KeyCode::Char('q'))),
                Some(Action::TypeSearch)
            );
            // Enter is compound: search submit vs confirm-from-selection.
            assert_eq!(
                f(m, Focus::Search, &press(KeyCode::Enter)),
                Some(Action::SubmitSearch)
            );
            assert_eq!(
                f(m, Focus::List, &press(KeyCode::Enter)),
                Some(Action::ConfirmSelected)
            );
            // i/Up/Down are compound: list actions vs typing branches.
            assert_eq!(
                f(m, Focus::List, &press(KeyCode::Char('i'))),
                Some(Action::InfoSelected)
            );
            assert_eq!(
                f(m, Focus::Search, &press(KeyCode::Char('i'))),
                Some(Action::TypeForced)
            );
            assert_eq!(
                f(m, Focus::List, &press(KeyCode::Up)),
                Some(Action::MovePrev)
            );
            assert_eq!(
                f(m, Focus::Search, &press(KeyCode::Char('k'))),
                Some(Action::TypeForced)
            );
            assert_eq!(
                f(m, Focus::List, &press(KeyCode::Down)),
                Some(Action::MoveNext)
            );
            assert_eq!(
                f(m, Focus::Search, &press(KeyCode::Char('j'))),
                Some(Action::TypeForced)
            );
        }
        // Mode-scoped rows.
        assert_eq!(
            f(Mode::Updates, Focus::List, &press(KeyCode::Char('U'))),
            Some(Action::UpgradeAll)
        );
        assert_eq!(
            f(Mode::Updates, Focus::Search, &press(KeyCode::Char('U'))),
            Some(Action::TypeSearch)
        );
        assert_eq!(
            f(Mode::Installed, Focus::List, &press(KeyCode::Char('o'))),
            Some(Action::ToggleOrphans)
        );
        assert_eq!(
            f(Mode::Search, Focus::List, &press(KeyCode::Char('o'))),
            Some(Action::TypeFromList)
        );
        for m in [Mode::Installed, Mode::Updates] {
            assert_eq!(
                f(m, Focus::List, &press(KeyCode::Char('c'))),
                Some(Action::CleanPreview)
            );
        }
        assert_eq!(
            f(Mode::Search, Focus::List, &press(KeyCode::Char('c'))),
            Some(Action::TypeFromList)
        );
        for code in [KeyCode::Delete, KeyCode::Backspace] {
            assert_eq!(
                f(Mode::Installed, Focus::List, &press(code)),
                Some(Action::RemoveSelected)
            );
        }
        for code in [
            KeyCode::Char('d'),
            KeyCode::Char('D'),
            KeyCode::Char('x'),
            KeyCode::Char('X'),
        ] {
            assert_eq!(
                f(Mode::Installed, Focus::List, &press(code)),
                Some(Action::RemoveSelected)
            );
        }
        assert_eq!(
            f(Mode::Installed, Focus::Search, &press(KeyCode::Char('x'))),
            Some(Action::TypeSearch)
        );
        for code in [
            (KeyCode::Char('r'), Action::ConfirmRefresh),
            (KeyCode::Char('R'), Action::ConfirmRefresh),
        ] {
            assert_eq!(f(Mode::Search, Focus::List, &press(code.0)), Some(code.1));
        }
    }

    /// Popup-layer resolution: q/Esc dismiss (and scoop up any upgrade
    /// warning), `?` only toggles Help, Enter/y confirm only on confirm
    /// dialogs, `n` closes everything, anything else is silently consumed.
    #[test]
    fn resolve_popup_matrix_matches_the_v010_arms() {
        fn p(class: PopupClass, ev: &KeyEvent) -> Option<Action> {
            resolve(Layer::Popup, Ctx::popup(class), ev)
        }
        for class in [PopupClass::Confirm, PopupClass::Closable, PopupClass::Help] {
            assert_eq!(
                p(class, &press(KeyCode::Esc)),
                Some(Action::ClosePopupDismiss)
            );
            assert_eq!(
                p(class, &press(KeyCode::Char('q'))),
                Some(Action::ClosePopupDismiss)
            );
            assert_eq!(
                p(class, &press(KeyCode::Char('n'))),
                Some(Action::ClosePopupSimple)
            );
            assert_eq!(
                p(class, &press(KeyCode::Char('N'))),
                Some(Action::ClosePopupSimple)
            );
            // Unclaimed keys are consumed-but-noop (old `_ => {}`).
            assert_eq!(p(class, &press(KeyCode::Char('x'))), None);
            assert_eq!(p(class, &press(KeyCode::Char('y'))), {
                if class == PopupClass::Confirm {
                    Some(Action::ConfirmActive)
                } else {
                    None
                }
            });
        }
        assert_eq!(
            p(PopupClass::Help, &press(KeyCode::Char('?'))),
            Some(Action::ToggleHelpPopup)
        );
        assert_eq!(p(PopupClass::Confirm, &press(KeyCode::Char('?'))), None);
        assert_eq!(p(PopupClass::Closable, &press(KeyCode::Char('?'))), None);
        assert_eq!(
            p(PopupClass::Confirm, &press(KeyCode::Enter)),
            Some(Action::ConfirmActive)
        );
        assert_eq!(
            p(PopupClass::Closable, &press(KeyCode::Enter)),
            Some(Action::ClosePopupSimple)
        );
        assert_eq!(
            p(PopupClass::Help, &press(KeyCode::Enter)),
            Some(Action::ClosePopupSimple)
        );
    }

    /// Modal-layer resolution: Esc unwinds exactly one sub-layer (editor →
    /// filter → modal close), the typing guard swallows unclaimed keys, and
    /// selection still moves while the filter is focused (the old
    /// `!editing` guard).
    #[test]
    fn resolve_modal_matrix_matches_the_v010_arms() {
        fn m(state: ModalState, ev: &KeyEvent) -> Option<Action> {
            resolve(
                Layer::Modal,
                Ctx::modal(state == ModalState::Editing, state == ModalState::Filtering),
                ev,
            )
        }
        // Editor level: everything else types into the box.
        assert_eq!(
            m(ModalState::Editing, &press(KeyCode::Esc)),
            Some(Action::ModalCancelEditor)
        );
        assert_eq!(
            m(ModalState::Editing, &press(KeyCode::Enter)),
            Some(Action::ModalCommitEditor)
        );
        assert_eq!(
            m(ModalState::Editing, &press(KeyCode::Char('S'))),
            Some(Action::ModalTypeEditor)
        );
        assert_eq!(
            m(ModalState::Editing, &press(KeyCode::Char(' '))),
            Some(Action::ModalTypeEditor)
        );
        assert_eq!(
            m(ModalState::Editing, &press(KeyCode::Up)),
            Some(Action::ModalTypeEditor)
        );
        // Filter level: Up/Down still move the selection (!editing); the
        // filter box takes typing; '/' re-focuses (old no-guard-on-check).
        assert_eq!(
            m(ModalState::Filtering, &press(KeyCode::Esc)),
            Some(Action::ModalClearFilter)
        );
        assert_eq!(
            m(ModalState::Filtering, &press(KeyCode::Enter)),
            Some(Action::ModalLeaveFilter)
        );
        assert_eq!(
            m(ModalState::Filtering, &press(KeyCode::Up)),
            Some(Action::ModalMovePrev)
        );
        assert_eq!(
            m(ModalState::Filtering, &press(KeyCode::Char('k'))),
            Some(Action::ModalMovePrev)
        );
        assert_eq!(
            m(ModalState::Filtering, &press(KeyCode::Down)),
            Some(Action::ModalMoveNext)
        );
        assert_eq!(
            m(ModalState::Filtering, &press(KeyCode::Char('/'))),
            Some(Action::ModalFilterFocus)
        );
        assert_eq!(
            m(ModalState::Filtering, &press(KeyCode::Char(' '))),
            Some(Action::ModalTypeFilter)
        );
        assert_eq!(
            m(ModalState::Filtering, &press(KeyCode::Char('x'))),
            Some(Action::ModalTypeFilter)
        );
        // List level: the modal keymap; unclaimed keys are silently consumed.
        assert_eq!(
            m(ModalState::Plain, &press(KeyCode::Esc)),
            Some(Action::ModalClose)
        );
        assert_eq!(
            m(ModalState::Plain, &press(KeyCode::Char('S'))),
            Some(Action::ModalClose)
        );
        assert_eq!(
            m(ModalState::Plain, &press(KeyCode::Up)),
            Some(Action::ModalMovePrev)
        );
        assert_eq!(
            m(ModalState::Plain, &press(KeyCode::Char('j'))),
            Some(Action::ModalMoveNext)
        );
        assert_eq!(
            m(ModalState::Plain, &press(KeyCode::Left)),
            Some(Action::ModalPrevGroup)
        );
        assert_eq!(
            m(ModalState::Plain, &press(KeyCode::Char('l'))),
            Some(Action::ModalNextGroup)
        );
        assert_eq!(
            m(ModalState::Plain, &press(KeyCode::Char('h'))),
            Some(Action::ModalPrevGroup)
        );
        assert_eq!(
            m(ModalState::Plain, &press(KeyCode::Right)),
            Some(Action::ModalNextGroup)
        );
        assert_eq!(
            m(ModalState::Plain, &press(KeyCode::Char('/'))),
            Some(Action::ModalFilterFocus)
        );
        assert_eq!(
            m(ModalState::Plain, &press(KeyCode::Enter)),
            Some(Action::ModalEnterRow)
        );
        assert_eq!(
            m(ModalState::Plain, &press(KeyCode::Char(' '))),
            Some(Action::ModalToggleBool)
        );
        assert_eq!(
            m(ModalState::Plain, &press(KeyCode::Char('r'))),
            Some(Action::ModalResetRow)
        );
        assert_eq!(
            m(ModalState::Plain, &press(KeyCode::Char('s'))),
            Some(Action::ModalSave)
        );
        assert_eq!(
            m(ModalState::Plain, &press(KeyCode::Char('e'))),
            Some(Action::ModalEditFile)
        );
        assert_eq!(m(ModalState::Plain, &press(KeyCode::Char('a'))), None);
    }

    /// Documented semantics (plan §3): Esc unwinds EXACTLY one layer at a
    /// time and `q` closes a popup, types in search, and quits only from
    /// list focus with nothing above it.
    #[test]
    fn esc_unwinds_one_layer_and_q_has_no_surprise_quits() {
        // Esc in a popup -> popup only (the frame Esc row never fires: layer
        // precedence, asserted structurally by the resolve layer).
        assert_eq!(
            resolve(
                Layer::Popup,
                Ctx::popup(PopupClass::Confirm),
                &press(KeyCode::Esc)
            ),
            Some(Action::ClosePopupDismiss)
        );
        // Esc in the modal -> modal (frame never consulted).
        assert_eq!(
            resolve(Layer::Modal, Ctx::modal(false, false), &press(KeyCode::Esc)),
            Some(Action::ModalClose)
        );
        // Esc in the editor -> editor, NOT the whole modal.
        assert_eq!(
            resolve(Layer::Modal, Ctx::modal(true, false), &press(KeyCode::Esc)),
            Some(Action::ModalCancelEditor)
        );
        // Esc searched -> list (frame only).
        assert_eq!(
            resolve(
                Layer::Frame,
                Ctx::frame(Mode::Search, Focus::Search),
                &press(KeyCode::Esc)
            ),
            Some(Action::ToggleToList)
        );
        // q: popup-close, search-type, list-quit — no accidental exits.
        assert_eq!(
            resolve(
                Layer::Popup,
                Ctx::popup(PopupClass::Confirm),
                &press(KeyCode::Char('q'))
            ),
            Some(Action::ClosePopupDismiss)
        );
        assert_eq!(
            resolve(
                Layer::Frame,
                Ctx::frame(Mode::Search, Focus::Search),
                &press(KeyCode::Char('q'))
            ),
            Some(Action::TypeSearch)
        );
        assert_eq!(
            resolve(
                Layer::Frame,
                Ctx::frame(Mode::Search, Focus::List),
                &press(KeyCode::Char('q'))
            ),
            Some(Action::Quit)
        );
        // Ctrl+C is the only anywhere-quit.
        assert_eq!(
            resolve(
                Layer::Frame,
                Ctx::frame(Mode::Updates, Focus::Search),
                &ctrl(KeyCode::Char('c'))
            ),
            Some(Action::Quit)
        );
    }
}
