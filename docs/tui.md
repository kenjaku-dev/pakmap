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
