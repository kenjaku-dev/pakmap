// SettingsScreen — every pakmap setting, visible from this door (plan §3):
// engine-rendered rows (value, default, origin chip, apply tag, danger copy),
// a filter box, group jumping, a write-target switch (user default; project
// only when the directory has one), per-row save/unset and the conflict
// banner. DANGER rows are always visible — grouped "Advanced" is last, never
// hidden. gui.theme is locked by the config override (D1): the row shows the
// lock and the only editor is "Set back to device".
import { useEffect, useState } from "react";
import { AlertTriangle, Copy, RotateCcw, X } from "lucide-react";
import type { ReactNode } from "react";
import {
  WRITE_TARGET_LABELS,
  isCustomized,
  type SettingsController,
} from "../lib/useSettings";
import { themeOverride } from "../lib/useTheme";
import type { SettingsEntry } from "../types";

export function SettingsScreen({
  ctl,
  onClose,
}: {
  ctl: SettingsController;
  onClose: () => void;
}) {
  const { data, loadError } = ctl;

  return (
    <div className="absolute inset-0 z-30 flex flex-col bg-background" data-testid="settings-screen">
      <header className="flex items-center gap-3 border-b border-border bg-card px-4 py-2">
        <h2 className="text-sm font-bold">Settings</h2>
        <span className="text-2xs text-muted-foreground">
          {data === null ? "…" : `${data.entries.length} keys — one engine, three doors`}
        </span>
        <input
          type="text"
          autoComplete="off"
          className="ml-2 w-52 rounded-md border border-border bg-card-muted px-2 py-1 text-xs text-foreground placeholder:text-muted-foreground focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent"
          placeholder="Filter settings…"
          value={ctl.query}
          onChange={(e) => ctl.setQuery(e.target.value)}
          aria-label="Filter settings"
          data-testid="settings-filter"
        />
        <button
          type="button"
          className="ml-auto flex cursor-pointer items-center gap-1 rounded-md px-1.5 py-1 text-xs text-muted-foreground transition-colors hover:text-foreground"
          onClick={ctl.reload}
          aria-label="Reload settings"
        >
          <RotateCcw size={14} aria-hidden="true" /> Reload
        </button>
        <button
          type="button"
          aria-label="Close settings"
          onClick={onClose}
          className="cursor-pointer rounded p-1.5 text-muted-foreground transition-colors hover:text-foreground"
        >
          <X size={16} aria-hidden="true" />
        </button>
      </header>

      {loadError !== null && (
        <div className="border-b border-border bg-card px-4 py-2 text-xs text-destructive" role="alert">
          {loadError.message}
        </div>
      )}

      {data !== null && (
        <WriteTargetStroke ctl={ctl} />
      )}

      {ctl.conflict && (
        <div
          className="flex items-center gap-3 border-b border-border bg-card px-4 py-2 text-xs"
          role="alert"
          data-testid="settings-conflict"
        >
          <AlertTriangle size={14} className="text-destructive" aria-hidden="true" />
          <span className="text-foreground">
            config changed on disk — the screen is stale
          </span>
          <button
            type="button"
            className="rounded border border-border px-2 py-0.5 text-2xs hover:bg-card-muted"
            onClick={ctl.reload}
          >
            Reload now
          </button>
        </div>
      )}

      <div className="flex min-h-0 flex-1">
        <GroupRail ctl={ctl} />
        <ScrollArea>
          {data === null && loadError === null && (
            <p className="p-4 text-xs text-muted-foreground" role="status">Loading settings…</p>
          )}
          {data !== null && (
            <>
              {data.warnings.length > 0 && (
                <div className="mx-4 mt-3 rounded-md border border-border bg-card-muted px-3 py-2 text-2xs text-muted-foreground">
                  <ul className="list-inside list-disc space-y-0.5">
                    {data.warnings.map((w) => (
                      <li key={w} className="break-words">{w}</li>
                    ))}
                  </ul>
                </div>
              )}
              {ctl.visible.map((e) => (
                <SettingsRow
                  key={e.key}
                  entry={e}
                  ctl={ctl}
                />
              ))}
              <EnvRows ctl={ctl} />
              <PathFooter ctl={ctl} />
            </>
          )}
        </ScrollArea>
      </div>
    </div>
  );
}

/* ------------------------------ structure ------------------------------- */

function ScrollArea({ children }: { children: ReactNode }) {
  return <main className="min-w-0 flex-1 overflow-y-auto pb-6">{children}</main>;
}

function GroupRail({ ctl }: { ctl: SettingsController }) {
  return (
    <nav
      aria-label="Setting groups"
      className="w-11/24 max-w-56 shrink-0 overflow-y-auto border-e border-border bg-card px-2 py-3"
    >
      <ul className="space-y-0.5">
        {ctl.groups.map((g) => (
          <li key={g}>
            <button
              type="button"
              onClick={() => ctl.setGroupFilter(g)}
              aria-pressed={ctl.groupFilter === g}
              className={`w-full rounded px-2 py-1 text-start text-xs transition-colors ${
                ctl.groupFilter === g
                  ? "bg-accent/15 font-semibold text-accent"
                  : "text-muted-foreground hover:bg-card-muted hover:text-foreground"
              }`}
            >
              {g}
            </button>
          </li>
        ))}
      </ul>
    </nav>
  );
}

function WriteTargetStroke({ ctl }: { ctl: SettingsController }) {
  if (ctl.data === null) return null;
  const projectAvailable = ctl.data.project_path !== null;
  return (
    <div className="flex items-center gap-2 border-b border-border bg-card-muted px-4 py-1.5 text-2xs text-muted-foreground">
      <span>Writes go to:</span>
      <select
        aria-label="Write target"
        value={ctl.target}
        onChange={(e) =>
          ctl.setTarget(e.target.value === "project" ? "project" : "user")
        }
        className="rounded border border-border bg-card px-1.5 py-0.5 text-2xs text-foreground"
      >
        <option value="user">{WRITE_TARGET_LABELS.user}</option>
        {projectAvailable && <option value="project">{WRITE_TARGET_LABELS.project}</option>}
      </select>
      {ctl.data.project_trusted && (
        <span className="rounded bg-card px-1.5 py-0.5">PAKMAP_TRUST_PROJECT=1 — project [behavior] allowed</span>
      )}
    </div>
  );
}

function EnvRows({ ctl }: { ctl: SettingsController }) {
  const rows = ctl.data?.env_rows ?? [];
  if (rows.length === 0) return null;
  return (
    <section aria-label="Environment knobs" className="mt-4 border-t border-border px-4 pt-3">
      <h3 className="text-2xs font-semibold uppercase tracking-wide text-muted-foreground">
        Session environment (read-only)
      </h3>
      <ul className="mt-2 space-y-1 text-xs">
        {rows.map((r) => (
          <li key={r.name} className="flex flex-wrap items-baseline gap-x-2">
            <code className="rounded bg-card-muted px-1 text-2xs">{r.name}</code>
            <span className="text-muted-foreground">{r.desc}</span>
          </li>
        ))}
      </ul>
    </section>
  );
}

function PathFooter({ ctl }: { ctl: SettingsController }) {
  const [copied, setCopied] = useState(false);
  const p = ctl.data?.user_path ?? null;
  if (p === null) return null;
  return (
    <footer className="mt-4 border-t border-border px-4 pt-3 text-2xs text-muted-foreground">
      <div className="flex items-center gap-2">
        <span>Config file:</span>
        <code className="select-all rounded bg-card-muted px-1.5 py-0.5 text-foreground">{p}</code>
        <button
          type="button"
          aria-label="Copy config file path"
          className="flex h-10 w-10 cursor-pointer items-center justify-center rounded bg-transparent text-muted-foreground transition-colors hover:text-foreground"
          onClick={() => {
            void navigator.clipboard?.writeText(p).catch(() => {});
            setCopied(true);
          }}
        >
          {copied ? <span className="text-2xs">copied</span> : <Copy size={13} aria-hidden="true" />}
        </button>
      </div>
      <p className="mt-1">
        The file stays the deepest door: edit it with any editor, then reload here.
      </p>
    </footer>
  );
}

/* --------------------------------- row ---------------------------------- */

function SettingsRow({ entry, ctl }: { entry: SettingsEntry; ctl: SettingsController }) {
  const err = ctl.rowErrors[entry.key] ?? null;
  const busy = ctl.busyKey === entry.key;
  const custom = isCustomized(entry);
  const modified = entry.value !== entry.default;

  return (
    <div
      className={`border-b border-border px-4 py-2.5 ${
        entry.danger === "danger"
          ? "border-s-2 border-s-destructive bg-destructive/5"
          : entry.danger === "caution"
            ? "border-s-2 border-s-accent-tertiary bg-card-muted/40"
            : ""
      }`}
      data-testid={`setting-row-${entry.key}`}
    >
      <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
        <code className="text-xs font-semibold">{entry.key}</code>
        <OriginChip entry={entry} />
        {entry.apply !== "immediate" && (
          <span className="rounded bg-card-muted px-1.5 text-2xs text-muted-foreground">
            applies {entry.apply === "next-launch" ? "at next launch" : "live preview"}
          </span>
        )}
        {entry.apply === "immediate" && modified && custom && (
          <span className="rounded bg-card-muted px-1.5 text-2xs text-muted-foreground" title="in-session effects depend on the key">
            applies on next operation
          </span>
        )}
        {entry.surfaces.tui && !entry.surfaces.gui && (
          <span className="rounded bg-card-muted px-1.5 text-2xs text-muted-foreground" title="this key feeds the terminal interface — it is still editable here">
            terminal-only
          </span>
        )}
        {modified && (
          <button
            type="button"
            disabled={busy}
            className="ms-auto flex h-10 items-center gap-1 rounded px-1.5 text-2xs text-muted-foreground transition-colors hover:text-foreground disabled:opacity-40"
            onClick={() => ctl.unset(entry.key)}
            aria-label={`Reset ${entry.key} to default`}
            data-testid={`reset-${entry.key}`}
          >
            <RotateCcw size={12} aria-hidden="true" /> default ({entry.default === "" ? "empty" : entry.default})
          </button>
        )}
      </div>

      <p className="mt-0.5 text-2xs text-muted-foreground">{entry.desc}</p>

      {entry.danger !== "" && (
        <DangerCopy entry={entry} />
      )}

      <div className="mt-1.5 flex items-center gap-2">
        <Control entry={entry} ctl={ctl} busy={busy} />
        {busy && <span className="text-2xs text-muted-foreground">saving…</span>}
      </div>

      {(err !== null || entry.warning !== null) && (
        <EntryFailures entry={entry} err={err} ctl={ctl} />
      )}
    </div>
  );
}

function DangerCopy({ entry }: { entry: SettingsEntry }) {
  const copy = DANGER_COPY[entry.key] ?? RISK_BY_KIND[entry.danger];
  return (
    <p className="mt-0.5 text-2xs text-destructive" role="note">
      <AlertTriangle size={10} className="me-1 inline align-baseline" aria-hidden="true" />
      {copy.text}
    </p>
  );
}

const RISK_BY_KIND: Record<string, { text: string }> = {
  caution: { text: "cautious-area setting — read the description before changing it" },
  danger: { text: "danger-area setting — changes affect system-mutating operations" },
};

const DANGER_COPY: Record<string, { text: string }> = {
  "behavior.readonly": {
    text: "readonly=true blocks EVERY installing/removing/upgrade/clean action behind this flag — it protects a test machine, but stonewalls your own pacmap until it is turned off",
  },
  "behavior.remove_noconfirm": {
    text: "skips the remove confirmation: pacman runs unattended — wrong inputs delete packages without a dialog",
  },
  "behavior.upgrade_noconfirm": {
    text: "skips the upgrade confirmation for the full repository+ AUR fleet in one non-interactive run",
  },
  "behavior.clean_noconfirm": {
    text: "skips the cache-clean confirmation — old package archives are pruned without asking",
  },
  "behavior.clean_clones_days": {
    text: "deletes AUR clone dirs older than N days on every clean — clones are re-fetchable caches, but the deletion is silent per clean; 0 keeps the feature off",
  },
  "behavior.makepkg_noconfirm": {
    text: "AUR makepkg runs non-interactive: dependency questions do not pause the build",
  },
  "behavior.refresh_noconfirm": {
    text: "skips confirmation of pacman -Sy database refreshes (sudo)",
  },
  "behavior.flatpak_noconfirm": {
    text: "skips confirmation for flatpak installs and removals",
  },
  "behavior.remove_flags": {
    text: "free-form pacman -R flags: -Rdd, -Rns and friends change how deep a removal cuts dependencies",
  },
  "behavior.aur_rpc": {
    text: "custom RPC mirror: direct, deterministic API compatibility is then trusted (not official archlinux.org) — safety checks rely on the remote",
  },
  "behavior.cache_dir": {
    text: "custom cache_dir changes where builds/archive walk — directory runs through pakmap's ownership checks before it is trusted",
  },
  "behavior.news_enabled": {
    text: "news warnings before risky updates arrive from archlinux.org — disabling it defeats the manual-intervention alert",
  },
};

function OriginChip({ entry }: { entry: SettingsEntry }) {
  const o = entry.origin;
  const cls =
    o.label === "user"
      ? "bg-accent/15 text-accent"
      : o.label === "project"
        ? "bg-accent-tertiary/15 text-accent-tertiary"
        : o.label === "default"
          ? "bg-card-muted text-muted-foreground"
          : "bg-card-muted text-foreground";
  const title =
    o.label === "user" || o.label === "project"
      ? o.label + " file: " + (o.path ?? "")
      : o.label === "device"
        ? "decided by the device (webview) — this key is an explicit default"
        : o.label;
  return (
    <span className={`rounded px-1.5 text-2xs ${cls}`} title={title} data-testid={`origin-${entry.key}`}>
      {o.label === "user" ? "your config" : o.label === "project" ? "project config" : o.label}
    </span>
  );
}

function EntryFailures({
  entry,
  err,
  ctl,
}: {
  entry: SettingsEntry;
  err: SettingsController["rowErrors"][string] | null;
  ctl: SettingsController;
}) {
  return (
    <div className="mt-1.5 rounded border border-destructive/40 bg-destructive/10 px-2 py-1.5 text-2xs text-destructive" role="alert">
      {err !== null && (
        <p>
          {err.message}
          {err.hint !== null && <span className="block text-muted-foreground">{err.hint}</span>}
        </p>
      )}
      {err === null && entry.warning !== null && <p>{entry.warning}</p>}
      {!entry.valid && err === null && (
        <p>
          the effective value is not valid for its kind — "Reset to default" clears it
        </p>
      )}
      {err?.canCreateUser === true && (
        <button
          type="button"
          className="mt-1 rounded border border-border bg-card px-2 py-0.5 text-2xs text-foreground"
          onClick={() => ctl.set(entry.key, entry.value, { create: true })}
        >
          Create your user config from the documented template
        </button>
      )}
    </div>
  );
}

/* ------------------------------- controls ------------------------------- */

function Control({
  entry,
  ctl,
  busy,
}: {
  entry: SettingsEntry;
  ctl: SettingsController;
  busy: boolean;
}) {
  if (entry.key === "gui.theme" && themeOverride() !== null) {
    return <ThemeLockRow ctl={ctl} busy={busy} />;
  }
  switch (entry.kind) {
    case "bool":
      return (
        <BoolControl entry={entry} ctl={ctl} busy={busy} />
      );
    case "enum":
      return <EnumControl entry={entry} ctl={ctl} busy={busy} />;
    case "u16":
    case "u32":
    case "u64":
    case "usize":
      return <TextControl entry={entry} ctl={ctl} inputMode="numeric" busy={busy} />;
    case "style":
    case "text":
    case "opt-text":
      return <TextControl entry={entry} ctl={ctl} busy={busy} />;
    case "pair":
      return <PairControl entry={entry} ctl={ctl} busy={busy} />;
  }
}

function BoolControl({
  entry,
  ctl,
  busy,
}: {
  entry: SettingsEntry;
  ctl: SettingsController;
  busy: boolean;
}) {
  const on = entry.value === "true" || entry.value === "1";
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={`${entry.key}: ${on ? "on" : "off"}`}
      disabled={busy}
      data-testid={`switch-${entry.key}`}
      className={`flex h-10 items-center gap-2 rounded-md border px-2 text-xs transition-colors ${
        on ? "border-accent text-accent" : "border-border text-muted-foreground"
      }`}
      onClick={() => ctl.set(entry.key, on ? "false" : "true")}
    >
      <span className={`h-2 w-2 rounded-full ${on ? "bg-accent" : "bg-muted-foreground/50"}`} aria-hidden="true" />
      {on ? "on" : "off"}
    </button>
  );
}

function EnumControl({
  entry,
  ctl,
  busy,
}: {
  entry: SettingsEntry;
  ctl: SettingsController;
  busy: boolean;
}) {
  return (
    <span className="flex items-center gap-2">
      <select
          value={entry.value}
          disabled={busy}
          aria-label={entry.key}
          data-testid={`enum-${entry.key}`}
          className="h-10 max-w-72 rounded-md border border-border bg-card px-2 text-xs"
          title={entry.value}
          onChange={(e) => ctl.set(entry.key, e.target.value)}
        >
          {entry.options.map((o) => (
            <option key={o.name} value={o.name}>
              {o.name}
              {o.desc !== "" ? ` — ${o.desc}` : ""}
            </option>
          ))}
        </select>
    </span>
  );
}

function TextControl({
  entry,
  ctl,
  busy,
  inputMode,
}: {
  entry: SettingsEntry;
  ctl: SettingsController;
  busy: boolean;
  inputMode?: "numeric";
}) {
  const [draft, setDraft] = useState(entry.value);
  // external reloads (conflict banner → reload) must refresh the draft
  useEffect(() => setDraft(entry.value), [entry.value, entry.key]);

  const commit = () => {
    const raw = draft.trim();
    if (raw === entry.value) return;
    if (entry.kind === "opt-text" && raw === "") {
      ctl.unset(entry.key); // empty = back to default for optionals
      return;
    }
    ctl.set(entry.key, raw);
  };
  const invalid =
    (inputMode === "numeric" && draft.trim() !== "" && !/^[0-9]+$/.test(draft.trim()));

  const badge =
    entry.clamp !== null
      ? `${entry.clamp[0]}..=${entry.clamp[1]}`
      : null;

  return (
    <span className="flex items-center gap-1.5">
      <input
        type="text"
        inputMode={inputMode}
        value={draft}
        disabled={busy}
        aria-label={entry.key}
        data-testid={`text-${entry.key}`}
        placeholder={
          entry.kind === "opt-text"
            ? "empty = default"
            : entry.default === ""
              ? "(default: empty)"
              : `default: ${entry.default}`
        }
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
          if (e.key === "Escape") setDraft(entry.value);
        }}
        className={`h-10 w-44 rounded-md border bg-card px-2 text-xs ${invalid ? "border-destructive" : "border-border"}`}
      />
      {invalid && <span className="text-2xs text-destructive">numbers only</span>}
      {badge !== null && (
        <span className="text-2xs text-muted-foreground" title="engine-validated range">
          {badge}
        </span>
      )}
    </span>
  );
}

function PairControl({
  entry,
  ctl,
  busy,
}: {
  entry: SettingsEntry;
  ctl: SettingsController;
  busy: boolean;
}) {
  const parts = entry.value.split(",");
  const [a, setA] = useState(parts[0] ?? "");
  const [b, setB] = useState(parts[1] ?? "");
  useEffect(() => {
    const [na, nb] = entry.value.split(",");
    setA(na ?? "");
    setB(nb ?? "");
  }, [entry.value, entry.key]);
  const commit = () => {
    const joined = `${a.trim()},${b.trim()}`;
    if (joined === entry.value) return;
    ctl.set(entry.key, joined);
  };
  return (
    <span className="flex items-center gap-1.5" onBlur={commit}>
      <input
        type="text"
        inputMode="numeric"
        value={a}
        disabled={busy}
        aria-label={`${entry.key} columns`}
        className="h-10 w-20 rounded-md border border-border bg-card px-2 text-xs"
        onChange={(e) => setA(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && commit()}
      />
      <span className="text-2xs text-muted-foreground" aria-hidden="true">×</span>
      <input
        type="text"
        inputMode="numeric"
        value={b}
        disabled={busy}
        aria-label={`${entry.key} rows`}
        className="h-10 w-20 rounded-md border border-border bg-card px-2 text-xs"
        onChange={(e) => setB(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && commit()}
      />
    </span>
  );
}

/** D1 lock: the toggle is inert; resetting the config key removes the pin at
 * the next launch (device behavior returns). The stored device preference
 * keeps existing untouched — it resumes the moment the pin leaves. */
function ThemeLockRow({
  ctl,
  busy,
}: {
  ctl: SettingsController;
  busy: boolean;
}) {
  const ov = themeOverride();
  return (
    <span className="flex flex-wrap items-center gap-2">
      {ov !== null ? (
        <span className="rounded bg-accent-tertiary/15 px-1.5 py-0.5 text-2xs text-accent-tertiary">
          pinned to {ov} by your config
        </span>
      ) : (
        <span className="rounded bg-accent-tertiary/15 px-1.5 py-0.5 text-2xs text-accent-tertiary">
          pinned by your config
        </span>
      )}
      <span className="text-2xs text-muted-foreground">
        the theme toggle is locked while [gui] theme overrides it — takes effect at relaunch
      </span>
      <button
        type="button"
        disabled={busy}
        data-testid="theme-unpin"
        className="h-10 rounded-md border border-border bg-card px-2 text-2xs"
        onClick={() => ctl.set("gui.theme", "device")}
      >
        Set back to device
      </button>
    </span>
  );
}
