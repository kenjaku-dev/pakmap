// Detail pane — title + badges row, meta fields, AUR popularity, description,
// dependency metadata (resolved for installed rows via package_info). The pane
// becomes a slide-in overlay below the content breakpoint (container query in
// App grid).
import { Copy, ExternalLink, X } from "lucide-react";
import { useEffect, useState, type ReactElement } from "react";
import { getPackageInfo, normalizeGuiError, openPackageUrl } from "../lib/ipc";
import type { OpRequest, PackageDto } from "../types";
import { badgeLabels } from "./badges";
import { PkgbuildSection } from "./PkgbuildSection";
import type { ConfirmSpec } from "./ConfirmDialog";

export interface Selection {
  leg: "repo" | "aur" | "flatpak";
  pkg: PackageDto;
}

/** AD action wiring: App owns the confirm dialog + ops runner. */
export interface PaneActions {
  readonly: boolean;
  busy: boolean;
  onAction: (request: OpRequest, confirm: ConfirmSpec) => void;
}

function Field({ k, v }: { k: string; v: React.ReactNode }) {
  if (v === null || v === "" || v === undefined) return null;
  return (
    <div className="flex gap-3 border-b border-border/60 py-1.5 text-xs last:border-none">
      <span className="w-28 shrink-0 text-muted-foreground">{k}</span>
      <span className="min-w-0 break-words">{v}</span>
    </div>
  );
}

/**
 * v0.9: package-URL row — label + "open in browser" / "copy" actions. The
 * open path goes through the Rust-validated command (untrusted AUR/package
 * metadata never reaches the opener unvalidated); a blocked URL surfaces the
 * rejection inline with copy as the fallback.
 */
function UrlField({ url, name }: { url: string; name: string }) {
  const [blocked, setBlocked] = useState<string | null>(null);
  const open = async () => {
    try {
      await openPackageUrl(name, url);
      setBlocked(null);
    } catch (e) {
      setBlocked(normalizeGuiError(e).message);
    }
  };
  return (
    <>
      <div className="flex gap-3 border-b border-border/60 py-1.5 text-xs">
        <span className="w-28 shrink-0 text-muted-foreground">url</span>
        <span className="min-w-0 flex-1 break-all">{url}</span>
        <span className="flex shrink-0 items-center gap-0.5">
          <button
            type="button"
            aria-label={`Open homepage: ${url}`}
            title="Open in browser"
            onClick={() => void open()}
            className="flex h-6 w-6 cursor-pointer items-center justify-center rounded text-muted-foreground transition-colors hover:text-foreground"
          >
            <ExternalLink size={14} aria-hidden="true" />
          </button>
          <button
            type="button"
            aria-label={`Copy homepage URL: ${url}`}
            title="Copy URL"
            onClick={() => void navigator.clipboard.writeText(url)}
            className="flex h-6 w-6 cursor-pointer items-center justify-center rounded text-muted-foreground transition-colors hover:text-foreground"
          >
            <Copy size={14} aria-hidden="true" />
          </button>
        </span>
      </div>
      {blocked && (
        <p className="mt-1.5 rounded border border-destructive/40 bg-destructive/10 px-2 py-1 text-2xs text-destructive">
          {blocked}
        </p>
      )}
    </>
  );
}

function fmtDate(epochSecs: number | null): string | null {
  if (epochSecs === null) return null;
  const d = new Date(epochSecs * 1000);
  if (Number.isNaN(d.getTime())) return String(epochSecs);
  return d.toISOString().slice(0, 10);
}

function DepList({ title, list }: { title: string; list: string[] | null }) {
  if (!list || list.length === 0) return null;
  return (
    <div className="mt-3">
      <h4 className="mb-1 text-nano font-semibold uppercase tracking-wider text-muted-foreground">
        {title}
        <span className="ml-1.5 font-normal normal-case">{list.length}</span>
      </h4>
      <ul className="flex flex-wrap gap-1.5">
        {list.map((d) => (
          <li key={d} className="rounded bg-card-muted px-1.5 py-0.5 text-nano">
            {d}
          </li>
        ))}
      </ul>
    </div>
  );
}

export function DetailPane({
  selection,
  actions,
  onClose,
}: {
  selection: Selection;
  actions?: PaneActions;
  onClose: () => void;
}) {
  const [full, setFull] = useState<PackageDto | null>(null);
  const [infoError, setInfoError] = useState<string | null>(null);

  useEffect(() => {
    setFull(null);
    setInfoError(null);
    if (!selection.pkg.installed) return; // remote rows carry their metadata
    let live = true;
    getPackageInfo(selection.pkg.name)
      .then((dto) => {
        if (live) setFull(dto);
      })
      .catch((e) => {
        if (live) setInfoError(normalizeGuiError(e).message);
      });
    return () => {
      live = false;
    };
  }, [selection.pkg.name, selection.pkg.installed]);

  const merged: PackageDto = full ?? selection.pkg;
  const votes = merged.votes ?? selection.pkg.votes;
  const badges = badgeLabels(merged).filter((b) => b !== "OOD" && b !== "ORPHAN");

  return (
    <aside
      aria-label="Package detail"
      tabIndex={-1}
      className="h-full overflow-y-auto border-l border-border bg-card outline-none"
    >
      <header className="sticky top-0 flex items-start justify-between gap-2 border-b border-border bg-card px-4 py-3">
        <div className="min-w-0">
          <h2 className="truncate text-sm font-semibold">{merged.name}</h2>
          <p className="mt-0.5 flex flex-wrap items-center gap-1.5 text-2xs">
            {merged.out_of_date !== null && (
              <span className="rounded bg-destructive/15 px-1.5 py-0.5 text-destructive">[OOD]</span>
            )}
            {merged.orphan && (
              <span className="rounded bg-destructive/15 px-1.5 py-0.5 text-destructive">[ORPHAN]</span>
            )}
            {badges.map((b) => (
              <span
                key={b}
                className="rounded bg-accent-tertiary/15 px-1.5 py-0.5 text-accent-tertiary"
              >
                [{b}]
              </span>
            ))}
          </p>
        </div>
        <button
          type="button"
          aria-label="Close detail"
          onClick={onClose}
          className="shrink-0 cursor-pointer rounded p-1 text-muted-foreground transition-colors hover:text-foreground"
        >
          <X size={14} aria-hidden="true" />
        </button>
      </header>

      <div className="px-4 py-3">
        <Field k="version" v={merged.version} />
        <Field k="repository" v={merged.repo} />
        <Field k="architecture" v={merged.arch} />
        {merged.url !== null && merged.url !== "" && (
          <UrlField url={merged.url} name={merged.name} />
        )}
        <Field
          k="popularity"
          v={
            votes !== null || merged.popularity !== null ? (
              <>
                {votes ?? "—"} votes{(merged.popularity !== null && ` · ${merged.popularity.toFixed(2)}`) ?? ""}
              </>
            ) : null
          }
        />
        <Field k="maintainer" v={merged.maintainer ?? "—"} />
        <Field k="install reason" v={merged.reason} />
        <Field k="last modified" v={fmtDate(merged.last_modified)} />

        {merged.description && (
          <p className="mt-3 text-xs leading-relaxed text-foreground/90">
            {merged.description}
          </p>
        )}
        {infoError && (
          <p className="mt-3 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-nano text-destructive">
            detail lookup failed: {infoError}
          </p>
        )}

        {actions !== undefined && (
          <OpActions merged={merged} actions={actions} />
        )}

        {merged.repo === "aur" && <PkgbuildSection name={merged.name} />}

        <DepList title="depends" list={merged.depends} />
        <DepList title="optional depends" list={merged.optdepends} />
        <DepList title="required by" list={merged.required_by} />
        <DepList title="optional for" list={merged.optional_for} />
      </div>
    </aside>
  );
}

/** Per-source action buttons with per-action risk copy. */
function OpActions({ merged, actions }: { merged: PackageDto; actions: PaneActions }) {
  const { readonly, busy, onAction } = actions;
  const disabled = readonly || busy;

  const isRepo = merged.repo !== "aur" && merged.repo !== "flatpak";
  const isAur = merged.repo === "aur";
  const isFlatpak = merged.repo === "flatpak";

  const buttons: ReactElement[] = [];

  if (isRepo && !merged.installed) {
    buttons.push(
      <button
        key="install"
        type="button"
        disabled={disabled}
        onClick={() =>
          onAction(
            { kind: "install_repo", args: { name: merged.name } },
            {
              title: `Install ${merged.name}`,
              body: [
                `Installs ${merged.name} ${merged.version} (pkexec pacman -S --needed --noconfirm).`,
                "The polkit dialog is the confirmation; the transaction runs non-interactively.",
              ],
              confirmLabel: "Install",
            },
          )
        }
        className="cursor-pointer rounded bg-accent-tertiary-strong px-3 py-1.5 text-xs font-semibold text-white transition-colors hover:bg-accent-tertiary/90 disabled:cursor-default disabled:opacity-50"
      >
        Install
      </button>,
    );
  }

  if (isAur && !merged.installed) {
    buttons.push(
      <button
        key="install-aur"
        type="button"
        disabled={disabled}
        onClick={() =>
          onAction(
            { kind: "install_aur", args: { name: merged.name } },
            {
              title: `Build & install ${merged.name} (AUR)`,
              body: [
                `Clones ${merged.name}'s PKGBUILD from the AUR, builds with makepkg as your user, then installs: pkexec pacman -U --noconfirm.`,
                "PKGBUILDs run arbitrary shell code during build — review it on aur.archlinux.org before installing. Cancel applies between phases (clone → build → install) only.",
              ],
              danger: true,
              confirmLabel: "Build & install",
            },
          )
        }
        className="cursor-pointer rounded bg-accent-tertiary-strong px-3 py-1.5 text-xs font-semibold text-white transition-colors hover:bg-accent-tertiary/90 disabled:cursor-default disabled:opacity-50"
      >
        Build & install (AUR)
      </button>,
    );
  }

  if (isFlatpak) {
    buttons.push(
      <button
        key="flatpak"
        type="button"
        disabled={disabled}
        onClick={() =>
          merged.installed
            ? onAction(
                { kind: "remove_flatpak", args: { app_id: merged.name } },
                {
                  title: `Remove ${merged.name}`,
                  body: [`flatpak remove --user --noninteractive ${merged.name}.`],
                  danger: true,
                  confirmLabel: "Remove",
                },
              )
            : onAction(
                { kind: "install_flatpak", args: { app_id: merged.name } },
                {
                  title: `Install ${merged.name}`,
                  body: [`flatpak install --user --noninteractive ${merged.name}.`],
                  confirmLabel: "Install",
                },
              )
        }
        className={
          merged.installed
            ? "cursor-pointer rounded border border-destructive/50 px-3 py-1.5 text-xs text-destructive transition-colors hover:bg-destructive/10 disabled:cursor-default disabled:opacity-50"
            : "cursor-pointer rounded bg-accent-tertiary-strong px-3 py-1.5 text-xs font-semibold text-white transition-colors hover:bg-accent-tertiary/90 disabled:cursor-default disabled:opacity-50"
        }
      >
        {merged.installed ? "Remove (flatpak)" : "Install (flatpak)"}
      </button>,
    );
  }

  if (merged.installed) {
    buttons.push(
      <button
        key="remove"
        type="button"
        disabled={disabled}
        onClick={() =>
          onAction(
            { kind: "remove", args: { name: merged.name } },
            {
              title: `Remove ${merged.name}`,
              body: [
                `Removes ${merged.name} and packages that depend on nothing else it pulled in (pkexec pacman -Rs --noconfirm).`,
                merged.reason === "dependency"
                  ? "This was installed as a dependency — removing may cascade."
                  : "Removable dependencies go too; your settings files are kept.",
              ],
              danger: true,
              confirmLabel: "Remove",
            },
          )
        }
        className="cursor-pointer rounded border border-destructive/50 px-3 py-1.5 text-xs text-destructive transition-colors hover:bg-destructive/10 disabled:cursor-default disabled:opacity-50"
      >
        Remove
      </button>,
    );
  }

  if (buttons.length === 0) return null;

  return (
    <div className="mt-3 border-t border-border/60 pt-3">
      <h4 className="mb-2 text-nano font-semibold uppercase tracking-wider text-muted-foreground">
        Actions
      </h4>
      <div className="flex flex-wrap gap-2">{buttons}</div>
      {readonly && (
        <p className="mt-1.5 text-2xs text-muted-foreground">
          readonly mode — actions disabled (behavior.readonly in config)
        </p>
      )}
      {merged.orphan && <p className="mt-1.5 text-2xs text-muted-foreground">orphaned dependency — nothing installed requires it anymore</p>}
    </div>
  );
}
