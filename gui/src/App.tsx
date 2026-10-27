// App — AD assembly: tabbed areas (Search | Updates | News) over the AC
// search screen. Operations (install/remove/upgrade/clean/refresh) are owned
// here: DetailPane and UpdatesScreen request (op, confirm-spec); App shows the
// ConfirmDialog, then dispatches through useOps; OpsBanner streams progress
// above the status stripe regardless of the active tab.
import { useCallback, useEffect, useMemo, useState } from "react";
import { kbd } from "./components/kbd";
import { ErrorCard } from "./components/ErrorCard";
import { FilterBar } from "./components/FilterBar";
import { SearchField } from "./components/SearchField";
import { StagedResults } from "./components/StagedResults";
import { DetailPane, type Selection } from "./components/DetailPane";
import { StatusStripe } from "./components/StatusStripe";
import { ConfirmDialog, type ConfirmSpec } from "./components/ConfirmDialog";
import { OpsBanner } from "./components/OpsBanner";
import { Tabs, TABS, type TabId } from "./components/Tabs";
import { ThemeToggle } from "./components/ThemeToggle";
import { BrandMark } from "./components/BrandMark";
import { UpdatesScreen } from "./components/UpdatesScreen";
import { NewsScreen } from "./components/NewsScreen";
import { ModalDialog } from "./components/dialog";
import { flattenRows } from "./state/searchReducer";
import { useSearch } from "./lib/useSearch";
import { useOps } from "./lib/useOps";
import { useTheme } from "./lib/useTheme";
import { getConfigFlags, mainWindowReady, onTrayCheckUpdates } from "./lib/ipc";
import type { OpRequest } from "./types";
import type { RowRef } from "./types";

export default function App() {
  const search = useSearch();
  const ops = useOps();
  const theme = useTheme();
  const [tab, setTab] = useState<TabId>("search");
  const [selection, setSelection] = useState<RowRef | null>(null);
  const [shortcuts, setShortcuts] = useState<{ open: boolean }>({ open: false });
  const [pending, setPending] = useState<{ req: OpRequest; spec: ConfirmSpec } | null>(null);
  const [readonly, setReadonly] = useState(false);

  useEffect(() => {
    let live = true;
    getConfigFlags().then((f) => {
      if (live) setReadonly(f.readonly);
    });
    // v0.9.1 §4 splash: this mount is the "webview JS runs" proof — reveal
    // the main window + close the splash here. Fire-and-forget: failure
    // leaves the Rust fallback timer to reveal the window anyway.
    void mainWindowReady().catch(() => {});
    return () => {
      live = false;
    };
  }, []);

  // v0.9 system tray: the tray's "Check for updates" menu item emits one
  // event; switching tabs re-mounts UpdatesScreen, which fetches on mount —
  // no extra refresh plumbing. The facade keeps the Tauri event API out of
  // the component tree (mock: no-op unlisten in browser E2E).
  useEffect(() => {
    let live = true;
    let unlisten: (() => void) | null = null;
    void onTrayCheckUpdates(() => setTab("updates")).then((u) => {
      if (live) unlisten = u;
      else u(); // component torn down before it armed → dispose immediately
    });
    return () => {
      live = false;
      unlisten?.();
    };
  }, []);

  const opBusy = ops.state.op !== null && ops.state.op.status === "running";

  // Fresh generation ⇒ the old selection belongs to a torn-down list.
  useEffect(() => setSelection(null), [search.state.generation]);

  const rows = useMemo(() => flattenRows(search.state), [search.state]);

  const picked = useMemo<Selection | null>(() => {
    if (selection === null) return null;
    const hit = rows.find((r) => r.leg === selection.stage && r.pkg.name === selection.name);
    return hit ? { leg: hit.leg, pkg: hit.pkg } : null;
  }, [rows, selection]);

  const move = useCallback(
    (delta: 1 | -1) => {
      if (rows.length === 0) return;
      const cur = selection
        ? rows.findIndex((r) => r.leg === selection.stage && r.pkg.name === selection.name)
        : -1;
      const next = rows[(cur + delta + rows.length) % rows.length];
      setSelection({ stage: next.leg, name: next.pkg.name });
    },
    [rows, selection],
  );

  // Selection highlight scrolls into view; focus stays in the search input
  // (palette behavior) and AT picks the row up via the listbox's
  // aria-activedescendant.
  useEffect(() => {
    if (selection === null) return;
    const key = CSS.escape(`${selection.stage}/${selection.name}`);
    document.querySelector(`[data-rowkey="${key}"]`)?.scrollIntoView({ block: "nearest" });
  }, [selection]);

  // Window-level hotkeys — scoped to the search tab for anything row-related
  // (arrows/Enter/Esc). "?" (shortcuts) and the tab hotkeys 1/2/3 are global.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "?") {
        e.preventDefault();
        e.stopPropagation(); // don't let a modal re-trigger or the query pick it up
        setShortcuts({ open: true });
        return;
      }
      const tabByKey = TABS.find((t) => e.key === t.key);
      if (tabByKey !== undefined) {
        // digits 1/2/3 jump tabs, but never while typing in an input
        const typing =
          document.activeElement instanceof HTMLInputElement ||
          document.activeElement instanceof HTMLTextAreaElement;
        if (!typing) {
          e.preventDefault();
          setTab(tabByKey.id);
        }
        return;
      }
      if (tab !== "search") return;

      const typing =
        document.activeElement instanceof HTMLInputElement ||
        document.activeElement instanceof HTMLTextAreaElement;
      if (e.key === "/" && !typing) {
        e.preventDefault();
        document.querySelector<HTMLInputElement>('input[aria-label="Search packages"]')?.focus();
      } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        // move() is a no-op when no rows exist — caret stays free until then
        move(e.key === "ArrowDown" ? 1 : -1);
      } else if (e.key === "Escape" && !typing && selection !== null) {
        setSelection(null);
      } else if (e.key === "Enter" && selection !== null) {
        // palette model: Enter on a live selection opens its detail (even while
        // the input is focused — arrows keep focus there); no selection means
        // the form submits a fresh search
        e.preventDefault();
        document.querySelector<HTMLElement>('aside[aria-label="Package detail"]')?.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [move, selection, tab]);

  // Central op entry: App shows the confirm, then dispatches to useOps.
  const requestAction = useCallback(
    (req: OpRequest, spec: ConfirmSpec) => {
      setPending({ req, spec });
    },
    [],
  );

  const confirmRequested = useCallback(() => {
    if (pending === null) return;
    const { req } = pending;
    setPending(null);
    void ops.run(req);
  }, [pending, ops]);

  const dismissOp = useCallback(() => {
    ops.dismiss();
  }, [ops]);

  return (
    <div className="flex h-screen flex-col">
      <header className="flex items-center gap-3 border-b border-border bg-card px-4 py-2">
        <BrandMark size={18} />
        <div className="select-none">
          <h1 className="text-sm font-bold tracking-tight text-foreground">pakmap</h1>
          <span className="sr-only">
            {search.summary ? `version ${search.summary.app_version}` : "loading"}
          </span>
        </div>
        <span className="shrink-0 rounded bg-card-muted px-1.5 text-2xs text-muted-foreground">
          {search.summary ? `v${search.summary.app_version}` : "…"}
        </span>
        <div className="ml-auto">
          <ThemeToggle mode={theme.mode} onChange={theme.setMode} />
        </div>
      </header>

      <Tabs active={tab} onChange={setTab} />

      {tab === "search" && (
        <div
          role="tabpanel"
          id="panel-search"
          aria-labelledby="tab-search"
          className="flex min-h-0 flex-1 flex-col"
        >
          <div className="flex items-center gap-4 border-b border-border bg-card px-4 py-2">
            <SearchField
              value={search.query}
              onInput={search.setQuery}
              onSubmit={search.run}
              onClear={search.clear}
            />
          </div>

          <FilterBar
            filters={search.filters}
            flatpakAvailable={search.summary?.flatpak_enabled ?? true}
            onSource={search.setSource}
            onToggle={search.toggle}
          />

          {search.state.error !== null && (
            <ErrorCard
              code={search.state.error.code}
              message={search.state.error.message}
              hint={search.state.error.hint}
            />
          )}

          <main
            className="@container relative flex min-h-0 flex-1 overflow-hidden"
            aria-label="Search results"
          >
            <StagedResults state={search.state} selected={selection} onSelect={setSelection} />

            {picked !== null && (
              <div className="@min-[1020px]:static absolute inset-y-0 right-0 z-10 @min-[1020px]:w-[min(24rem,40%)] w-85 max-w-full shadow-lg">
                <DetailPane
                  selection={picked}
                  actions={{ readonly, busy: opBusy, onAction: requestAction }}
                  onClose={() => setSelection(null)}
                />
              </div>
            )}

            {picked !== null && (
              <button
                type="button"
                className="@min-[1020px]:hidden absolute inset-0 z-[5] bg-black/40"
                aria-label="Close detail overlay"
                tabIndex={-1}
                onClick={() => setSelection(null)}
              />
            )}
          </main>
        </div>
      )}

      {tab === "updates" && (
        <div
          role="tabpanel"
          id="panel-updates"
          aria-labelledby="tab-updates"
          className="min-h-0 flex-1 overflow-hidden"
        >
          <UpdatesScreen api={{ requestAction, readonly }} />
        </div>
      )}

      {tab === "news" && (
        <div
          role="tabpanel"
          id="panel-news"
          aria-labelledby="tab-news"
          className="min-h-0 flex-1 overflow-hidden"
        >
          <NewsScreen />
        </div>
      )}

      {ops.state.op !== null && (
        <OpsBanner ops={ops.state} onCancel={() => void ops.cancel()} onDismiss={dismissOp} />
      )}

      <StatusStripe state={search.state} />

      {pending !== null && (
        <ConfirmDialog
          spec={pending.spec}
          onConfirm={confirmRequested}
          onClose={() => setPending(null)}
        />
      )}

      {shortcuts.open && <ShortcutsOverlay onClose={() => setShortcuts({ open: false })} />}
    </div>
  );
}

function ShortcutsOverlay({ onClose }: { onClose: () => void }) {
  return (
    <ModalDialog label="Keyboard shortcuts" onClose={onClose}>
      <h2 className="mb-3 text-sm font-semibold">Keyboard shortcuts</h2>
      <ul className="space-y-2 text-xs text-muted-foreground">
        <li className="flex justify-between gap-4">
          <span>Focus search</span>
          <span>{kbd("/")}</span>
        </li>
        <li className="flex justify-between gap-4">
          <span>Search now (skip debounce)</span>
          <span>{kbd("Enter")}</span>
        </li>
        <li className="flex justify-between gap-4">
          <span>Move between rows</span>
          <span>
            {kbd("↑")} {kbd("↓")}
          </span>
        </li>
        <li className="flex justify-between gap-4">
          <span>Open detail on selected row</span>
          <span>{kbd("Enter")}</span>
        </li>
        <li className="flex justify-between gap-4">
          <span>Close detail / clear search</span>
          <span>{kbd("Esc")}</span>
        </li>
        <li className="flex justify-between gap-4">
          <span>Switch area</span>
          <span>
            {kbd("1")} {kbd("2")} {kbd("3")}
          </span>
        </li>
        <li className="flex justify-between gap-4">
          <span>Toggle regex / installed / orphans</span>
          <span>click chips</span>
        </li>
      </ul>
      <p className="mt-3 text-2xs text-muted-foreground">Esc closes this dialog.</p>
    </ModalDialog>
  );
}
