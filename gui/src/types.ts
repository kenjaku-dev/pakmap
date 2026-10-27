// Shared wire types — mirror the Rust structs 1:1 (serde serializes field
// names as-is, so snake_case here; see docs/gui-architecture.md).

export interface PackageDto {
  name: string;
  version: string;
  description: string | null;
  repo: string;
  arch: string | null;
  url: string | null;
  installed: boolean;
  votes: number | null;
  popularity: number | null;
  out_of_date: number | null;
  maintainer: string | null;
  last_modified: number | null;
  depends: string[] | null;
  optdepends: string[] | null;
  required_by: string[] | null;
  optional_for: string[] | null;
  reason: string | null;
  orphan: boolean;
}

export interface ConfigSummary {
  app_version: string;
  limit: number;
  source: string;
  timeout_secs: number;
  flatpak_enabled: boolean;
  custom_aur_rpc: boolean;
  debounce_ms: number;
}

export interface GuiError {
  code: string;
  message: string;
  hint: string | null;
}

export type SourceChoice = "all" | "repo" | "aur" | "flatpak";

export interface SearchFilters {
  source: SourceChoice;
  aur_by: string | null;
  use_regex: boolean;
  installed_only: boolean;
  orphans_only: boolean;
  limit: number | null;
}

/** stage mirrors `tui::SearchStage::{Repo, Aur, Flatpak, Done}` */
export type SearchStage = "repo" | "aur" | "flatpak" | "done";

/** the three stream legs (everything but the `done` marker) */
export type LegStage = Exclude<SearchStage, "done">;

/** Event payload of `search://stage` (Tauri Channel, docs contract). */
export interface SearchEvent {
  generation: number;
  stage: SearchStage;
  packages: PackageDto[];
}

/** Reducer-visible row key: (stage, name) is unique within a generation. */
export interface RowRef {
  stage: Exclude<SearchStage, "done">;
  name: string;
}

// ---------------------------------------------------------------------------
// AD operations surface (docs/gui-architecture.md §operations)
// ---------------------------------------------------------------------------

export type OpRequest =
  | { kind: "install_repo"; args: { name: string } }
  | { kind: "install_flatpak"; args: { app_id: string } }
  | { kind: "install_aur"; args: { name: string } }
  | { kind: "remove"; args: { name: string } }
  | { kind: "remove_flatpak"; args: { app_id: string } }
  | { kind: "remove_orphans" }
  | { kind: "upgrade" }
  | { kind: "refresh" }
  | { kind: "clean"; args: { keep: number } };

/** `op://channel` messages — one Channel per operation. */
export type OpMessage =
  | { kind: "progress"; op_id: number; phase: string; line: string }
  | {
      kind: "done";
      op_id: number;
      ok: boolean;
      exit_code: number | null;
      error: GuiError | null;
    };

/** One available update, installed version → target version. */
export interface PkgUpdateDto {
  name: string;
  old_version: string;
  new_version: string;
  repo: string;
  out_of_date: boolean;
}

/** Both update legs degrade independently (see UpdatesResult comment). */
export interface UpdatesResult {
  repo: PkgUpdateDto[];
  aur: PkgUpdateDto[];
  repo_err: string | null;
  aur_err: string | null;
  orphans: string[];
}

export interface NewsItemDto {
  title: string;
  link: string;
  pub_date: string | null;
  pub_epoch: number | null;
  needs_action: boolean;
}

/** Behavior flags the frontend needs up-front to disable mutating actions. */
export interface ConfigFlags {
  readonly: boolean;
  flatpak_enabled: boolean;
}

/** v0.9: wire shape of `get_pkgbuild` (docs/gui-architecture.md §pkgbuild).
 * `source` drives the review states: cached clone / fresh fetch / needs fetch. */
export interface PkgbuildResult {
  name: string;
  source: "cache" | "fetched" | "none";
  content: string | null;
  truncated: boolean;
  diff: string | null;
  fetch_ok: boolean;
}

/** TUI --stats parity; cache sizes pre-formatted by the backend. */
export interface StatsDto {
  explicit: number;
  dependencies: number;
  orphans: number;
  updates_repo: number;
  updates_aur: number;
  updates_flatpak: number;
  flatpak_installed: number;
  cache_pacman_bytes: number;
  cache_pakmap_bytes: number;
  cache_pacman_fmt: string;
  cache_pakmap_fmt: string;
  /** epoch secs of the last executed AUR check; null = never checked. */
  aur_checked_at: number | null;
}
