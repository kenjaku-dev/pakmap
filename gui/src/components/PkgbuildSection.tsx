// PKGBUILD review — v0.9 (docs/gui-architecture.md §pkgbuild): the AUR trust
// review happens in the GUI now, closing the "PKGBUILD stays a TUI surface"
// divergence. Display-only by design: no build button in this section (the
// build entry stays in OpActions with its risk copy). Content renders as
// React text nodes (never dangerouslySetInnerHTML); since v0.9.1 it passes
// through a dependency-free tokenizer (pkgbuildHighlight) for coloring,
// which cannot alter the reviewed bytes (lossless invariant, unit-tested).
// Cache-first: mounting only reads the local clone; fetching is an explicit
// user action backed by a bounded, cancellable-by-timeout clone/pull.
import { Copy, ExternalLink, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { highlightPkgbuild, PKG_TOKEN_CLASS, type PkgToken } from "../lib/pkgbuildHighlight";
import { getPkgbuild, normalizeGuiError, openExternal } from "../lib/ipc";
import type { PkgbuildResult } from "../types";

/** v0.9.1 §3: the plain-`<pre>` grew a dependency-free regex tokenizer.
 * Still display-only + escaped (React text nodes, no dangerouslySetInnerHTML),
 * and the diff panel stays plain text by design. */
function PkgbuildBody({ content }: { content: string }) {
  const tokens = highlightPkgbuild(content);
  return (
    <pre
      data-testid="pkgbuild-highlighted"
      className="max-h-56 mt-1 overflow-auto rounded bg-card-muted px-2 py-1.5 font-mono text-nano leading-snug whitespace-pre"
    >
      {tokens.map((t: PkgToken, i: number) =>
        t.kind === "plain" ? (
          t.text
        ) : (
          <span key={i} data-token-kind={t.kind} className={PKG_TOKEN_CLASS[t.kind]}>
            {t.text}
          </span>
        ),
      )}
    </pre>
  );
}

export function PkgbuildSection({ name }: { name: string }) {
  const [res, setRes] = useState<PkgbuildResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [fetching, setFetching] = useState(false);
  const [fetchFailed, setFetchFailed] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    // Offline cache read on every AUR selection (per contract: no auto-fetch).
    let live = true;
    setLoading(true);
    setFetchFailed(false);
    setErr(null);
    getPkgbuild(name, null)
      .then((r) => {
        if (live) {
          setRes(r);
          setLoading(false);
        }
      })
      .catch((e) => {
        if (live) {
          setErr(normalizeGuiError(e).message);
          setLoading(false);
        }
      });
    return () => {
      live = false;
    };
  }, [name]);

  const fetchNow = () => {
    setFetching(true);
    setErr(null);
    getPkgbuild(name, true)
      .then((r) => {
        setRes(r);
        setFetchFailed(!r.fetch_ok);
        setFetching(false);
      })
      .catch((e) => {
        setErr(normalizeGuiError(e).message);
        setFetching(false);
      });
  };

  const busy = loading || fetching;
  const content = res?.content ?? null;

  return (
    <section aria-label="PKGBUILD review" className="mt-3 border-t border-border/60 pt-3">
      <div className="mb-2 flex items-center justify-between gap-2">
        <h4 className="text-nano font-semibold uppercase tracking-wider text-muted-foreground">
          PKGBUILD
        </h4>
        {res?.source === "fetched" && (
          <span className="rounded bg-accent-tertiary/15 px-1.5 py-0.5 text-2xs text-accent-tertiary">
            fresh from AUR
          </span>
        )}
      </div>

      {busy ? (
        <div className="mt-1 space-y-1.5">
          <span className="sr-only">Reviewing PKGBUILD…</span>
          <div className="h-3 w-3/4 animate-pulse rounded bg-card-muted" aria-hidden="true" />
          <div className="h-3 w-full animate-pulse rounded bg-card-muted" aria-hidden="true" />
          <div className="h-3 w-2/3 animate-pulse rounded bg-card-muted" aria-hidden="true" />
        </div>
      ) : content !== null ? (
        <PkgbuildBody content={content} />
      ) : (
        <p className="text-2xs text-muted-foreground">
          no cached clone for this package — fetch it from the AUR to review what a build would run
        </p>
      )}

      {res?.truncated && (
        <p className="mt-1.5 rounded border border-destructive/40 bg-destructive/10 px-2 py-1 text-2xs text-destructive">
          PKGBUILD shown truncated (256KB cap) — review the full file on aur.archlinux.org
        </p>
      )}
      {fetchFailed && res?.source === "cache" && !busy && (
        <p className="mt-1.5 text-2xs text-muted-foreground">
          remote refresh failed — showing the cached copy
        </p>
      )}
      {res?.diff && (
        <details className="mt-2 rounded border border-border/60 px-2 py-1.5">
          <summary className="cursor-pointer text-nano font-semibold uppercase tracking-wider text-muted-foreground">
            Changes since last install
          </summary>
          <pre className="mt-1.5 overflow-auto font-mono text-nano leading-snug whitespace-pre">
            {res.diff}
          </pre>
        </details>
      )}
      {err && (
        <p className="mt-1.5 rounded-lg border border-destructive/40 bg-destructive/10 px-2 py-1 text-nano text-destructive">
          {err}
        </p>
      )}

      <div className="mt-2 flex flex-wrap gap-2">
        <button
          type="button"
          onClick={() => void openExternal(`https://aur.archlinux.org/packages/${name}`, name)}
          className="flex cursor-pointer items-center gap-1 rounded border border-border px-2 py-1 text-nano transition-colors hover:bg-card-muted"
        >
          <ExternalLink size={12} aria-hidden="true" />
          Open on AUR
        </button>
        {content !== null && (
          <button
            type="button"
            onClick={() => void navigator.clipboard.writeText(content)}
            className="flex cursor-pointer items-center gap-1 rounded border border-border px-2 py-1 text-nano transition-colors hover:bg-card-muted"
          >
            <Copy size={12} aria-hidden="true" />
            Copy PKGBUILD
          </button>
        )}
        {content === null && !busy && (
          <button
            type="button"
            onClick={fetchNow}
            className="flex cursor-pointer items-center gap-1 rounded border border-border px-2 py-1 text-nano transition-colors hover:bg-card-muted"
          >
            <RefreshCw size={12} aria-hidden="true" />
            Fetch from AUR
          </button>
        )}
      </div>
    </section>
  );
}
