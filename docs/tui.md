# pakmap TUI — design & accessibility notes

This document tracks the terminal UI's design decisions. It grows phase by
phase (see the five-phase plan). Phase 1 establishes the measurable, accessible
baseline.

## Phase 1 — measurement & visual baseline (0.12)

### Contrast

The default palette is audited against the xterm reference RGB for the 16 base
ANSI colors (test: `default_palette_meets_wcag_contrast_on_black` in
`src/tui/style.rs`). Targets on a black background:

- Primary text (`text`, `text_dim`, `version`): >= 4.5:1.
- Secondary text / UI components (`help`, `tab_normal`, borders, idle status):
  >= 3:1.
- Composite pairs (selected row, active tab): >= 4.5:1.

The old `dark_gray` (#555) dim roles measured ~2.8:1 and failed the floor, so
`text_dim`, `help`, `tab_normal`, and `border_unfocused` now default to `gray`
(#e5e5e5). `highlight_bg` stays `dark_gray` because the selected-row foreground
is white bold (high contrast on that background).

**Caveat:** ANSI color names are emulator/theme-dependent. This audit pins the
*choice* of default names against a reference palette; it is not a claim of
universal WCAG conformance on every terminal theme. Users who need exact colors
can override any `[theme]` key with `#RRGGBB` or a 256-color index.

### Wider color support

`[theme]` style values accept, in addition to the 16 named colors:

- Truecolor: `#RRGGBB` (e.g. `text_dim = "#b0b0b0"`).
- 256-color index: `0`..`255` (e.g. `highlight_bg = "236"`).

Malformed values fall back to the terminal default (`Reset`), never a panic.
Both foreground and background positions work: `"#ff8800 on #101010 bold"`.

### Focus indicator

The results list mirrors the search box's focus ring: the focused region draws
`border_focused`, the idle one `border_unfocused`. Focus is therefore visible
without relying on color alone (the same pair swaps on the search box).

### Glyph set (`tui.icons`)

`auto` (default) | `unicode` | `ascii`.

- `auto` uses the emoji/braille set unless the session looks ASCII-only:
  `NO_COLOR`, `TERM=linux`, or `LC_ALL=C`/`POSIX`.
- `ascii` forces plain-ASCII tabs (`[1]Search` …) and an ASCII spinner
  (`| / - \`), with no emoji or braille anywhere in the chrome.
- `unicode` forces the emoji/braille set even under `NO_COLOR`.

This replaces the old dead `width < 60` heuristic (the default floor is 60).

### Golden snapshots

`tests/tui_golden.rs` renders twelve hermetic screens through `TestBackend` and
pins a trimmed text grid plus an FNV-1a hash over every cell's symbol and
fg/bg/underline/modifier encoding. Regenerate deliberately:

```text
UPDATE_TUI_SNAPSHOTS=1 cargo test --test tui_golden
```

Screens cover 60x14, 80x24, 120x30 (preview), 200x50, the help/info/confirm/
message popups, the settings modal, and a NO_COLOR + ASCII frame.

## Phase 2 — information architecture & honesty (0.12)

### The status line reports state; the footer teaches keys

Boot statuses are short state sentences (`Ready — type a query`,
`Installed — empty filter shows all`, `Updates — empty filter shows all`).
The keymap lives in the footer legend and the `?` popup, not duplicated in the
status strip.

### Width-aware footer

`keys::footer_legend_for_width(mode, focus, width)` fits the generated legend
to the terminal. When it overflows, the lowest-priority tokens are evicted
(never split) in this order: primary action (`Enter`) and search survive
longest, then mode cycling, help, quit, then the detail trio (`i`/`F`/`S`),
then everything else. A `?:more` marker advertises the overflow. At full width
the output is byte-identical to `footer_legend` (the v0.10 drift test still
pins that string).

### Region titles

The filter box title names the region only — ` Search `, ` Filter installed `,
` Filter updates `. Focus is already the border ring; instructions belong to
the footer/help, so titles no longer clip and never carry stale copy.

### Honest source progress

A failed search leg is a failure, not a zero: `LegView.error` carries a short
reason written by the worker (`repo`/`aur`/`flatpak`), the status strip renders
`aur ✗`, and the final status appends `— aur (timeout) failed; showing the
rest`. Healthy legs keep their counts.

### Honest upgrade-one dialog

Pressing Enter on an Updates row opens `Confirm upgrade` with the real
transition `old → new` (parsed from the row's update description) and the
command the flow actually runs: `sudo pacman -S …` for repo rows,
`flatpak update …` for Flatpak, and the PKGBUILD note for AUR. It no longer
reuses the install dialog's copy.

### Visible sort ring

`T` cycles the sort; any non-default sort appends `· sort:name|repo|version`
to the results title. Arrival stays unmarked so the v0.10 census titles remain
byte-exact.

### Spinner alphabet

`tui.tick_chars` now drives the TUI status spinner as well as the CLI progress
bar (non-empty override wins; otherwise the glyph set picks braille vs ASCII).
`tui.spinner_template` is documented as CLI-only.

### Empty states

Empty results explain the state and the next action (`No results — try fewer
words, or F to filter the current list`, `No installed match — clear the
filter or Tab to Updates`, `System up to date`).

## Phase 3 — responsiveness & layering (0.12)

### Row density (`tui.row_density`)

`auto` (default) | `comfortable` | `compact`. `auto` resolves against the
whole frame: compact below 70 columns **or** 20 rows, wide at 120+ columns,
comfortable in between. Compact draws one line per row (chip + name + version
+ signals) — roughly twice as many packages per screen. Comfortable keeps the
description on a second line. Wide adds an `arch · upd YYYY-MM-DD` metadata
column in the description line, but only when the preview pane is off: the
visible fact sheet stays the single source of those details.

### Content-sized popups

`layout::popup_rect_fit` replaces percentage-height boxes: width comes from
the first config number (min 20, max 100 / `area-4`), height is the wrapped
`Paragraph::line_count` plus the border rows, clamped to `area-4`. The second
number of `popup_info`/`popup_confirm`/`popup_message`/`popup_help` is kept
for config compatibility but no longer sets the height — the popup grows for
long copy and shrinks for short copy instead of clipping.

### Popup scrolling

Long help/info/message popups scroll with `j/k`, `PageUp/PageDown`, `g/G`
(one shared `popup_scroll`, reset on close). A dim `▲ 1/2 ▼` page indicator
occupies the last inner row only while scrolling. The entire `?` help and an
arbitrarily long error message are reachable at 80×24 and 60×14.

### Responsive settings modal

The modal caps at 100×40. Below 70 inner columns the `apply` column goes;
below 60 the `origin` column and the category rail go too. Below 20 rows the
theme preview strip is dropped and the list + detail bands get the space —
usable at the 60×14 floor.

### Preview pane default

`preview_pane` now defaults to **on**, but only renders at 120+ columns (the
raised `PREVIEW_MIN_W`); at 80 the `i` modal remains the detail surface. Set
`tui.preview_pane = false` to force it off everywhere.

### Floor

The default floor stays 60×14 (the minimum clamp remains 40×10). The
too-small warning now explains the recovery: resize or lower
`tui.floor_width`/`tui.floor_height`.

## Phase 4 — interaction & feedback (0.12)

### Honest filter cursor

The inline filter line uses the same `layout::tail` + `input.visual_cursor()`
math as the search box. The fake end-of-line `▏` is gone; the native cursor
sits at the real visual position even in the middle of long/CJK text.

### Faster navigation

`PageUp`/`PageDown` move a logical page (visible list rows minus borders),
`Home`/`End` join `g`/`G` at the edges. All four are in `BINDINGS`, the
generated footer and the `?` help — no second keymap source.

### Optional mouse

`tui.mouse = false` by default (text selection keeps working). When enabled:
wheel moves the selection, a click selects a row, two clicks within 400 ms on
the same row open the info popup. Capture is released before a child process
owns the TTY and re-established on reentry; released again on exit. Most
terminals recover text selection with Shift+drag.

### Status with meaning

`set_status(kind, text)` is the one entry point. `StatusKind::{Info, Success,
Warn, Error}` maps to `status_loading/idle/success/warn/error` from the theme
catalog. A success flash expires after 5 s and returns to the mode's idle
sentence; an error persists until the next key or Esc. ✓/✗ prefixes carry the
meaning without color; the mono palette uses bold for the semantic roles.

### Operation log & query history

`L` opens a bounded, memory-only log of the last 20 operations (time, action,
target, result, duration). `H` opens the last 20 session queries: Enter re-runs
the selected one, `d` deletes it. Neither writes to disk.

### Command palette

`Ctrl+P` opens a palette generated from `keys::BINDINGS`: type to filter,
`j`/`k` select, Enter runs the highlighted command, Esc closes. Every entry is
an existing `Action` — the palette cannot drift into a stale command list.

### Timed operation feedback

Before handing the TTY to pacman/makepkg/paccache/flatpak the status says
`Running … — terminal handed over`. Afterwards the status and popup report
`✓ Installed foo in 12.4s` (or `✗ Failed foo: …`) and the operation lands in
the log with its measured duration.

## Phase 5 — identity & final polish (0.12)

### Row grid

`CHIP_W = 8` keeps the origin chip a fixed cell width; `name_cap(interior)`
reserves a minimum version window plus the widest AUR tail, so the version
column starts at the same x on every row (no ragged gaps). `tui.column_header`
(default off) adds a `repo / name / version / signals` header in **wide density
only**; compact rows never get one.

### One glyph set

`✓ success`, `! warning`, `✗ error`, `~ busy`, `▲/▼ scroll`, `▸ selected`.
`chrome::glyph` resolves every one through `tui.icons`: ASCII mode uses
`+ ! x ~ ^ v >` so the critical path never contains emoji. The tabs, spinner,
status legs, results title, popups, settings modal and list marker all call
the same resolver.

### Palette & identity

The default roles are documented in `docs/settings.md` and commented in
`config.example.toml`, together with a ready high-contrast block (white text
and bold semantic roles). `theme.preset` is deliberately deferred: a shortcut
would have to expand into the existing layered keys, and that churn would
fight the origin/override contract — the commented preset is zero-risk and
copies straight into the same file.

### Visual rhythm

All popups use one block vocabulary (`layout::overlay_block`) and titles of
the same shape (` Confirm … `, ` Help (?) `, ` Message `, ` Operations log `).
Spacing comes from the layout module (one border depth, one row gap); motion
is limited to the spinner and scroll indicator.

### Final review matrix

| Screen | Check |
|---|---|
| 60×14 | compact rows, floor message explains recovery, settings usable |
| 80×24 | comfortable rows, full help reachable by scrolling |
| 120×30 | preview pane on by default, wide columns when it is off |
| 200×50 | capped 100×40 modal, capped popup width |
| NO_COLOR / ASCII | mono palette, plain glyphs, no emoji in the grid |

The signed review record lives in `context.md` (entry CD).
