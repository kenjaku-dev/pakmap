// Real IPC gateway — the only place `invoke`/`Channel` are touched.
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  ConfigFlags,
  ConfigSummary,
  GuiError,
  NewsItemDto,
  OpMessage,
  OpRequest,
  PackageDto,
  PkgbuildResult,
  SearchEvent,
  SearchFilters,
  StatsDto,
  UpdatesResult,
} from "../types";

export type SearchEventSink = (event: SearchEvent) => void;

/**
 * Kick off a staged search. Returns the generation immediately; staged
 * SearchEvents (repo → aur → flatpak → done) stream through the channel.
 */
export async function startSearch(
  query: string,
  filters: SearchFilters,
  sink: SearchEventSink,
): Promise<{ generation: number }> {
  const channel = new Channel<SearchEvent>();
  channel.onmessage = sink;
  try {
    const generation = await invoke<number>("search", { query, filters, onEvent: channel });
    return { generation };
  } catch (err) {
    // A command error rejects with the serialized GuiError envelope.
    throw normalizeGuiError(err);
  }
}

export function cancelSearch(): Promise<number> {
  return invoke<number>("cancel_search");
}

export function getConfigSummary(): Promise<ConfigSummary> {
  return invoke<ConfigSummary>("get_config_summary");
}

/** Detail resolution for installed rows (deps/reason via localdb). */
export async function getPackageInfo(name: string): Promise<PackageDto> {
  try {
    return await invoke<PackageDto>("package_info", { name });
  } catch (err) {
    throw normalizeGuiError(err);
  }
}

// ---------------------------------------------------------------------------
// AD operations surface
// ---------------------------------------------------------------------------

/** Kick an operation off. Progress/Done stream through the channel sink. */
export async function runOp(
  request: OpRequest,
  sink: (msg: OpMessage) => void,
): Promise<number> {
  const channel = new Channel<OpMessage>();
  channel.onmessage = sink;
  try {
    return await invoke<number>("run_op", { op: request, onEvent: channel });
  } catch (err) {
    throw normalizeGuiError(err);
  }
}

/** Phase-boundary cancel. False = op unknown/already terminal. */
export function cancelOperation(opId: number): Promise<boolean> {
  return invoke<boolean>("cancel_operation", { opId });
}

/** Free the backend cancel-flag entry after a terminal banner is dismissed. */
export function dropOperation(opId: number): Promise<void> {
  return invoke<void>("drop_operation", { opId });
}

export function getConfigFlags(): Promise<ConfigFlags> {
  return invoke<ConfigFlags>("get_config_flags");
}

/** v0.9.1 §4 splash: mount-time reveal of the (visible:false) main window +
 * splash close. Fire-and-forget on the frontend side (never blocks UI),
 * Rust-side failure still surfaces the window via the 5s fallback timer. */
export function mainWindowReady(): Promise<void> {
  return invoke<void>("main_window_ready");
}

export async function getUpdates(): Promise<UpdatesResult> {
  return invoke<UpdatesResult>("get_updates");
}

export async function getNews(): Promise<NewsItemDto[]> {
  return invoke<NewsItemDto[]>("get_news");
}

export async function getStats(): Promise<StatsDto> {
  return invoke<StatsDto>("get_stats");
}

/**
 * v0.9: cache-first PKGBUILD review (read-only display; fetch=true performs
 * the bounded clone/pull refresh on a blocking backend worker). `fetch` is
 * `null` for the plain cache read.
 */
export async function getPkgbuild(name: string, fetch: boolean | null): Promise<PkgbuildResult> {
  try {
    return await invoke<PkgbuildResult>("get_pkgbuild", { name, fetch });
  } catch (err) {
    throw normalizeGuiError(err);
  }
}

/**
 * Open a package homepage in the system browser after Rust-side validation
 * (scheme gate + credential/control-char rejection). Unsafe URLs reject with
 * `invalid_url` — the UI falls back to copy.
 */
export async function openPackageUrl(name: string, url: string): Promise<void> {
  try {
    await invoke<void>("open_package_url", { name, url });
  } catch (err) {
    throw normalizeGuiError(err);
  }
}

/**
 * v0.9 system tray → frontend bridge: the tray's "Check for updates" menu
 * item emits this event; the app switches to the Updates tab (which re-mounts
 * and refreshes naturally). Returns the unlisten function.
 */
export async function onTrayCheckUpdates(handler: () => void): Promise<() => void> {
  const unlisten = await listen("tray:check-updates", () => handler());
  return unlisten;
}

/**
 * v0.9.3 Phase 3: ALL external opens — package pages and news links — route
 * through the same Rust command (`open_package_url`), so every open crosses
 * the shared `pakmap::safety::url` gate before the OS browser is involved.
 * The direct opener-plugin path is gone from this gateway on purpose: there
 * is no open call that can skip Rust. `name` is only the audit label
 * (package name or news-row title); unsafe URLs reject with `invalid_url` —
 * the UI shows copy-only fallback rows instead.
 */
export async function openExternal(url: string, name: string): Promise<void> {
  try {
    await invoke<void>("open_package_url", { name: name.trim() || "external", url });
  } catch (err) {
    throw normalizeGuiError(err);
  }
}

export function normalizeGuiError(err: unknown): GuiError {
  if (
    typeof err === "object" &&
    err !== null &&
    "code" in err &&
    typeof (err as { code: unknown }).code === "string"
  ) {
    return err as GuiError;
  }
  return { code: "unknown", message: String(err), hint: null };
}
