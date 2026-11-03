// Search orchestration hook: owns the run identity chain, the per-call event
// sink binding, debounced auto-run (config `tui.debounce_ms`, clamped like the
// TUI), and the immediate Enter run. The reducer holds the TUI invariants.
import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import {
  emptyFilters,
  initialSearchState,
  searchReducer,
  type SearchState,
} from "../state/searchReducer";
import { cancelSearch, getConfigSummary, normalizeGuiError, startSearch } from "./ipc";
import type { ConfigSummary, GuiError, SearchFilters, SourceChoice } from "../types";

export interface UseSearch {
  state: SearchState;
  query: string;
  setQuery: (q: string) => void;
  filters: SearchFilters;
  setSource: (s: SourceChoice) => void;
  toggle: (k: "use_regex" | "installed_only" | "orphans_only") => void;
  run: (immediate?: boolean) => void;
  /** clear query + detail + tear streaming legs down */
  clear: () => void;
  summary: ConfigSummary | null;
  isError: boolean;
  /** refetch after a settings write so limit/source/debounce re-arm */
  reloadSummary: () => void;
}

export function useSearch(): UseSearch {
  const [state, dispatch] = useReducer(searchReducer, initialSearchState);
  const [query, setQuery] = useState("");
  const [filters, setFilters] = useState<SearchFilters>(emptyFilters);
  const [summary, setSummary] = useState<ConfigSummary | null>(null);
  /** bumped by the settings screen after a config write → summary refetch */
  const [reloadSummaryFlag, setReloadSummaryFlag] = useState(0);
  const reloadSummary = useCallback(() => setReloadSummaryFlag((n) => n + 1), []);

  // run identity chain + "which run is current" latch
  const nextIdentity = useRef(1);
  const pending = useRef(false);

  useEffect(() => {
    let alive = true;
    const fetchSummary = () => {
      getConfigSummary()
        .then((s) => {
          if (alive) setSummary(s);
        })
        .catch(() => {
          // fail-open: defaults mirror the lib's (config is single source of truth)
          if (alive) {
            setSummary({
              app_version: "unknown",
              limit: 50,
              source: "all",
              timeout_secs: 30,
              flatpak_enabled: true,
              custom_aur_rpc: false,
              debounce_ms: 400,
            });
          }
        });
    };
    fetchSummary();
    return () => {
      alive = false;
    };
  }, [reloadSummaryFlag]);

  const execute = useCallback(
    (q: string, f: SearchFilters) => {
      const identity = nextIdentity.current++;
      pending.current = true;
      const carried = f.installed_only || f.orphans_only;
      dispatch({
        type: "start",
        identity,
        query: q,
        filters: f,
        carried,
        at: Date.now(),
      });
      const myIdentity = identity;
      startSearch(q, f, (event) => {
        // Bound to THIS call's channel: events from a superseded run can only
        // reach the reducer through their own (stale) identity — no ghost mix.
        dispatch({ type: "stage", identity: myIdentity, event, at: Date.now() });
      }).catch((err: GuiError) => {
        dispatch({ type: "error", error: normalizeGuiError(err) });
      });
    },
    [],
  );

  const triggerRef = useRef(execute);
  const queryRef = useRef(query);
  const filtersRef = useRef(filters);
  queryRef.current = query;
  filtersRef.current = filters;

  // Debounced auto-run — debounce semantics from config, clamped lib-side.
  useEffect(() => {
    if (!pending.current) return; // first render: nothing typed yet
    const deb = Math.max(50, summary?.debounce_ms ?? 400);
    const t = setTimeout(() => {
      const q = queryRef.current.trim();
      if (q === "") return; // empty query is intentionally inert (per TUI)
      triggerRef.current(q, filtersRef.current);
    }, deb);
    return () => clearTimeout(t);
  }, [query, filters, summary?.debounce_ms]);

  const run = useCallback(() => {
    const q = queryRef.current.trim();
    if (q === "") return;
    triggerRef.current(q, filtersRef.current);
  }, []);

  const clear = useCallback(() => {
    pending.current = false;
    setQuery("");
    cancelSearch().catch(() => undefined);
    dispatch({ type: "reset" });
  }, []);

  const setSource = useCallback((s: SourceChoice) => {
    setFilters((f) => ({ ...f, source: s }));
  }, []);

  const toggle = useCallback((k: "use_regex" | "installed_only" | "orphans_only") => {
    setFilters((f) => ({ ...f, [k]: !f[k] }));
  }, []);

  return {
    state,
    query,
    setQuery: (q: string) => {
      pending.current = true;
      setQuery(q);
    },
    filters,
    setSource,
    toggle,
    run,
    clear,
    summary,
    isError: state.error !== null,
    reloadSummary,
  };
}
