// UpdatesScreen — phase AD parity: the TUI "Updates" screen as a tab. Both
// legs load independently and degrade independently (a failed leg renders its
// own error card; the good leg still shows). Upgrade-all is ONE transaction
// (the TUI's semantics); per-row update runs the single-package install op.
// Orphan removal rides on the same screen (batch, count from get_updates).
import { useEffect, useState } from "react";
import { getStats, getUpdates, normalizeGuiError } from "../lib/ipc";
import type { OpRequest, StatsDto, UpdatesResult } from "../types";
import { ErrorCard } from "./ErrorCard";
import { QuickHint } from "./states";

export interface UpdatesApi {
  /** centralized action entry — App runs the confirm then op.run */
  requestAction: (req: OpRequest, confirm: { title: string; body: string[]; danger?: boolean; confirmLabel: string }) => void;
  readonly: boolean;
}

/** Relative age for the "AUR checked" chip (B6): coarse on purpose — the
 * memo TTL is 5 min, so anything past an hour is honest about staleness. */
function relAgo(epochSecs: number): string {
  const d = Math.max(0, Math.floor(Date.now() / 1000) - epochSecs);
  if (d < 4 * 60) return "just now";
  if (d < 60 * 60) return `${Math.floor(d / 60)}m ago`;
  if (d < 24 * 60 * 60) return `${Math.floor(d / (60 * 60))}h ago`;
  return `${Math.floor(d / (24 * 60 * 60))}d ago`;
}

export function UpdatesScreen({ api }: { api: UpdatesApi }) {
  const [data, setData] = useState<UpdatesResult | null>(null);
  const [stats, setStats] = useState<StatsDto | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    setData(null);
    setError(null);
    getUpdates()
      .then((u) => {
        if (live) setData(u);
      })
      .catch((e) => {
        if (live) setError(normalizeGuiError(e).message);
      });
    getStats()
      .then((s) => {
        if (live) setStats(s);
      })
      .catch(() => {
        /* the chip row is additive — stats failing never blocks updates */
      });
    return () => {
      live = false;
    };
  }, []);

  if (error !== null) {
    return <SingleCard code="updates_failed" message={error} />;
  }
  if (data === null) {
    return (
      <div className="flex h-full items-center justify-center">
        <p role="status" className="text-xs text-muted-foreground">
          Checking for updates…
        </p>
      </div>
    );
  }

  const repoCount = data.repo.length;
  const aurCount = data.aur.length;
  const total = repoCount + aurCount;
  const ro = api.readonly;

  return (
    <div className="mx-auto h-full max-w-3xl overflow-y-auto px-4 py-4">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <h2 className="text-sm font-semibold">Updates</h2>
        <div className="flex flex-wrap gap-2">
          <button
            type="button"
            disabled={ro || total === 0}
            onClick={() =>
              api.requestAction(
                { kind: "upgrade" },
                {
                  title: "Full system upgrade",
                  body: [
                    `One transaction applies all ${total} update(s) (pkexec pacman -Syu --noconfirm).`,
                    "The polkit dialog is the confirmation. A system transaction is never killed mid-flight.",
                  ],
                  danger: true,
                  confirmLabel: "Upgrade system",
                },
              )
            }
            className="cursor-pointer rounded bg-destructive-strong px-3 py-1.5 text-xs font-semibold text-white transition-colors hover:bg-destructive/90 disabled:cursor-default disabled:opacity-50"
          >
            Upgrade all ({total})
          </button>
          <button
            type="button"
            disabled={ro}
            onClick={() =>
              api.requestAction(
                { kind: "refresh" },
                {
                  title: "Refresh databases",
                  body: ["Refreshes the sync databases (pkexec pacman -Sy)."],
                  confirmLabel: "Refresh",
                },
              )
            }
            className="cursor-pointer rounded border border-border px-3 py-1.5 text-xs transition-colors hover:bg-card-muted disabled:cursor-default disabled:opacity-50"
          >
            Refresh dbs
          </button>
          {data.orphans.length > 0 && (
            <button
              type="button"
              disabled={ro}
              onClick={() =>
                api.requestAction(
                  { kind: "remove_orphans" },
                  {
                    title: "Remove orphans",
                    body: [
                      `Removes ${data.orphans.length} orphaned package(s): ${data.orphans.join(", ")}.`,
                      "Orphans are packages installed as dependencies that no installed package requires.",
                    ],
                    danger: true,
                    confirmLabel: "Remove all",
                  },
                )
              }
              className="cursor-pointer rounded border border-destructive/50 px-3 py-1.5 text-xs text-destructive transition-colors hover:bg-destructive/10 disabled:cursor-default disabled:opacity-50"
            >
              Remove orphans ({data.orphans.length})
            </button>
          )}
          <button
            type="button"
            disabled={ro}
            onClick={() =>
              api.requestAction(
                { kind: "clean", args: { keep: 2 } },
                {
                  title: "Clean package cache",
                  body: [
                    "Keeps the newest 2 versions of each package (pkexec paccache -r -k2).",
                    "Requires the pacman-contrib package (paccache); the polkit dialog is the confirmation.",
                  ],
                  confirmLabel: "Clean cache",
                },
              )
            }
            className="cursor-pointer rounded border border-border px-3 py-1.5 text-xs transition-colors hover:bg-card-muted disabled:cursor-default disabled:opacity-50"
          >
            Clean cache
          </button>
        </div>
      </div>

      {ro && (
        <QuickHint text="readonly mode is enabled — mutating actions are disabled (behavior.readonly in ~/.config/pakmap/config.toml)" />
      )}

      {stats !== null && (
        <div
          data-testid="stats-chips"
          className="mt-4 flex flex-wrap gap-x-4 gap-y-1 text-nano text-muted-foreground"
          aria-label="System statistics"
        >
          <span>
            installed{" "}
            <span className="text-foreground" style={{ fontVariantNumeric: "tabular-nums" }}>
              {stats.explicit + stats.dependencies}
            </span>{" "}
            <span aria-hidden="true">·</span>{" "}
            <span className="text-muted-foreground">
              {stats.explicit} explicit / {stats.dependencies} deps
            </span>
          </span>
          <span aria-hidden="true" className="text-border">
            │
          </span>
          <span>
            flatpak{" "}
            <span className="text-foreground" style={{ fontVariantNumeric: "tabular-nums" }}>
              {stats.flatpak_installed}
            </span>
          </span>
          <span aria-hidden="true" className="text-border">
            │
          </span>
          <span>
            pacman cache{" "}
            <span className="text-foreground" style={{ fontVariantNumeric: "tabular-nums" }}>
              {stats.cache_pacman_fmt}
            </span>
          </span>
          {stats.aur_checked_at !== null && (
            <>
              <span aria-hidden="true" className="text-border">
                │
              </span>
              <span>
                AUR checked{" "}
                <span className="text-foreground" style={{ fontVariantNumeric: "tabular-nums" }}>
                  {relAgo(stats.aur_checked_at)}
                </span>
              </span>
            </>
          )}
        </div>
      )}

      {data.repo_err !== null && (
        <ErrorCard code="repo_updates_failed" message={data.repo_err} hint="check mirror connectivity; repo updates will retry on reload" />
      )}
      {data.aur_err !== null && (
        <ErrorCard code="aur_updates_failed" message={data.aur_err} hint="AUR RPC unreachable or unsupported mirror; AUR updates will retry on reload" />
      )}

      {data.repo.length > 0 && (
        <section>
          <h3 className="mb-1.5 mt-4 text-nano font-semibold uppercase tracking-wider text-muted-foreground">
            Official repositories <span className="ml-1 normal-case">{data.repo.length}</span>
          </h3>
          <ul role="list" aria-label="Official repository updates" className="divide-y divide-border/60">
            {data.repo.map((u) => (
              <li key={u.name} className="flex items-center justify-between gap-3 py-2">
                <span className="min-w-0">
                  <span className="block truncate text-xs font-medium">{u.name}</span>
                  <span className="text-2xs text-muted-foreground">
                    {u.old_version} → {u.new_version}
                  </span>
                </span>
                <span className="flex shrink-0 items-center gap-2">
                  <span className="rounded bg-accent-tertiary/15 px-1.5 py-0.5 text-2xs text-accent-tertiary">
                    [{u.repo === "aur" ? "AUR" : u.repo.toUpperCase()}]
                  </span>
                  {u.out_of_date && (
                    <span className="rounded bg-destructive/15 px-1.5 py-0.5 text-2xs text-destructive">
                      [OOD]
                    </span>
                  )}
                  <button
                    type="button"
                    disabled={ro}
                    aria-label={`Update ${u.name}`}
                    onClick={() =>
                      api.requestAction(
                        { kind: "install_repo", args: { name: u.name } },
                        {
                          title: `Update ${u.name}`,
                          body: [
                            `${u.old_version} → ${u.new_version} (pkexec pacman -S --needed --noconfirm ${u.name}).`,
                          ],
                          confirmLabel: "Update package",
                        },
                      )
                    }
                    className="cursor-pointer rounded border border-border px-2 py-0.5 text-nano transition-colors hover:bg-card-muted disabled:cursor-default disabled:opacity-50"
                  >
                    Update
                  </button>
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}

      {data.aur.length > 0 && (
        <section>
          <h3 className="mb-1.5 mt-4 text-nano font-semibold uppercase tracking-wider text-muted-foreground">
            AUR <span className="ml-1 normal-case">{data.aur.length}</span>
          </h3>
          <ul role="list" aria-label="AUR updates" className="divide-y divide-border/60">
            {data.aur.map((u) => (
              <li key={u.name} className="flex items-center justify-between gap-3 py-2">
                <span className="min-w-0">
                  <span className="block truncate text-xs font-medium">{u.name}</span>
                  <span className="text-2xs text-muted-foreground">
                    {u.old_version} → {u.new_version}
                  </span>
                </span>
                <span className="shrink-0 text-2xs text-muted-foreground">
                  update via search & detail pane (AUR builds run per package)
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}

      {total === 0 && (
        <p className="mt-8 text-center text-xs text-muted-foreground">
          Everything is up to date. Run Refresh dbs after a config/mirror change.
        </p>
      )}
    </div>
  );
}

function SingleCard({ code, message }: { code: string; message: string }) {
  return (
    <div className="p-4">
      <ErrorCard code={code} message={message} hint={null} />
    </div>
  );
}
