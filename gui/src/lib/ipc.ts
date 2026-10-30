// IPC facade — the app's single import point for backend calls.
// In mock mode (VITE_PAKMAP_MOCK_IPC=1, dev/E2E only) everything routes through
// the scripted gateway; in production the env replacement is a compile-time
// false and the mock module is tree-shaken out of the bundle.
import * as realGateway from "./tauriGateway";
import * as mockGateway from "./mockIpc";
import { normalizeGuiError } from "./tauriGateway";
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
export { normalizeGuiError };
export type { GuiError };

const MOCK = import.meta.env.VITE_PAKMAP_MOCK_IPC === "1";
const gw = MOCK ? mockGateway : realGateway;

export function startSearch(
  query: string,
  filters: SearchFilters,
  sink: (event: SearchEvent) => void,
): Promise<{ generation: number }> {
  return gw.startSearch(query, filters, sink);
}

export function cancelSearch(): Promise<number> {
  return gw.cancelSearch();
}

export function getConfigSummary(): Promise<ConfigSummary> {
  return gw.getConfigSummary();
}

export function getPackageInfo(name: string): Promise<PackageDto> {
  return gw.getPackageInfo(name);
}

// ---------------------------------------------------------------------------
// AD operations surface
// ---------------------------------------------------------------------------

export function runOp(
  request: OpRequest,
  sink: (msg: OpMessage) => void,
): Promise<number> {
  return gw.runOp(request, sink);
}

export function cancelOperation(opId: number): Promise<boolean> {
  return gw.cancelOperation(opId);
}

export function dropOperation(opId: number): Promise<void> {
  return gw.dropOperation(opId);
}

export function getConfigFlags(): Promise<ConfigFlags> {
  return gw.getConfigFlags();
}

/** Splash reveal (v0.9.1 §4): see tauriGateway/mock notes. */
export function mainWindowReady(): Promise<void> {
  return gw.mainWindowReady();
}

export function getUpdates(): Promise<UpdatesResult> {
  return gw.getUpdates();
}

export function getNews(): Promise<NewsItemDto[]> {
  return gw.getNews();
}

export function getStats(): Promise<StatsDto> {
  return gw.getStats();
}

/**
 * Open an external link via the shared Rust opener command (v0.9.3: package
 * pages AND news links through one gate; `auditName` labels the open in the
 * Rust log). Unsafe URLs reject with `invalid_url`.
 */
export function openExternal(url: string, auditName: string): Promise<void> {
  return gw.openExternal(url, auditName);
}

// ---------------------------------------------------------------------------
// v0.9 features — PKGBUILD review / package-URL opener / tray seam
// ---------------------------------------------------------------------------

/** Cache-first PKGBUILD review; fetch=true triggers the bounded refresh. */
export function getPkgbuild(name: string, fetch: boolean | null): Promise<PkgbuildResult> {
  return gw.getPkgbuild(name, fetch);
}

/** Open a package homepage after Rust-side validation (unsafe → invalid_url). */
export function openPackageUrl(name: string, url: string): Promise<void> {
  return gw.openPackageUrl(name, url);
}

/** Tray "Check for updates" → app switches to the Updates tab. */
export function onTrayCheckUpdates(handler: () => void): Promise<() => void> {
  return gw.onTrayCheckUpdates(handler);
}

/** normalizeGuiError is re-exported above — shared by both gateways. */

export type { SearchEvent };
