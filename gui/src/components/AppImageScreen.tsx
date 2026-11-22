// AppImageScreen — local AppImage desktop integration (v0.11.1).
// No catalog, no AppImage execution: the backend reads metadata with
// unsquashfs and tracks every file it writes so removal is exact.
import { useEffect, useState, type FormEvent } from "react";
import {
  getAppImageEntries,
  integrateAppImage,
  installAppImage,
  normalizeGuiError,
  removeAppImage,
} from "../lib/ipc";
import type { AppImageEntry, GuiError } from "../types";
import { ConfirmDialog, type ConfirmSpec } from "./ConfirmDialog";
import { ErrorCard } from "./ErrorCard";

export function AppImageScreen() {
  const [entries, setEntries] = useState<AppImageEntry[] | null>(null);
  const [error, setError] = useState<GuiError | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const [path, setPath] = useState("");
  const [localName, setLocalName] = useState("");
  const [url, setUrl] = useState("");
  const [sha256, setSha256] = useState("");
  const [urlName, setUrlName] = useState("");
  const [allowUnverified, setAllowUnverified] = useState(false);
  const [pendingRemove, setPendingRemove] = useState<AppImageEntry | null>(null);

  function reload(): Promise<void> {
    return getAppImageEntries()
      .then(setEntries)
      .catch((e: unknown) => setError(normalizeGuiError(e)));
  }

  useEffect(() => {
    void reload();
  }, []);

  async function perform(
    action: () => Promise<AppImageEntry>,
    success: (entry: AppImageEntry) => string,
  ): Promise<void> {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const entry = await action();
      setNotice(success(entry));
      await reload();
    } catch (e) {
      setError(normalizeGuiError(e));
    } finally {
      setBusy(false);
    }
  }

  const onIntegrate = (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    if (!path.trim()) return;
    void perform(
      () => integrateAppImage(path.trim(), localName.trim() || null),
      (entry) => `Integrated ${entry.name} (${entry.id})`,
    );
  };

  const onInstall = (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    if (!url.trim()) return;
    void perform(
      () => installAppImage(url.trim(), sha256.trim() || null, allowUnverified, urlName.trim() || null),
      (entry) => `Installed ${entry.name} (${entry.id})`,
    );
  };

  const onRemove = (entry: AppImageEntry) => setPendingRemove(entry);

  const confirmRemove = () => {
    const entry = pendingRemove;
    setPendingRemove(null);
    if (entry === null) return;
    void (async () => {
      setBusy(true);
      setError(null);
      setNotice(null);
      try {
        await removeAppImage(entry.id);
        setNotice(`Removed ${entry.name}`);
        await reload();
      } catch (e) {
        setError(normalizeGuiError(e));
      } finally {
        setBusy(false);
      }
    })();
  };

  const removeSpec: ConfirmSpec | null = pendingRemove && {
    title: `Remove ${pendingRemove.name}`,
    body: [
      "This deletes the integrated AppImage, its desktop entry and its icon.",
      "The original AppImage file you integrated is not touched.",
    ],
    danger: true,
    confirmLabel: "Remove",
  };

  return (
    <div className="mx-auto h-full max-w-3xl overflow-y-auto px-4 py-4">
      <h2 className="text-sm font-semibold">AppImages</h2>
      <p className="mt-1 text-xs text-muted-foreground">
        Integrate a file you already have. The AppImage is never executed during integration;
        SHA-256 is required for URL installs. Needs squashfs-tools.
      </p>

      {error !== null && (
        <div className="mt-3">
          <ErrorCard code={error.code} message={error.message} hint={error.hint} />
        </div>
      )}
      {notice !== null && (
        <p role="status" className="mt-3 text-xs text-foreground">
          {notice}
        </p>
      )}

      <section aria-labelledby="appimage-integrate-heading" className="mt-4 border-t border-border pt-3">
        <h3 id="appimage-integrate-heading" className="text-xs font-semibold">
          Integrate a local file
        </h3>
        <form onSubmit={onIntegrate} className="mt-2 flex flex-wrap items-end gap-2">
          <div className="min-w-60 flex-1">
            <label htmlFor="appimage-path" className="block text-2xs text-muted-foreground">
              AppImage path
            </label>
            <input
              id="appimage-path"
              value={path}
              onChange={(e) => setPath(e.target.value)}
              disabled={busy}
              placeholder="/home/me/Downloads/App.AppImage"
              className="mt-1 w-full rounded border border-border bg-card px-2 py-1 text-xs"
            />
          </div>
          <div>
            <label htmlFor="appimage-local-name" className="block text-2xs text-muted-foreground">
              Name (optional)
            </label>
            <input
              id="appimage-local-name"
              value={localName}
              onChange={(e) => setLocalName(e.target.value)}
              disabled={busy}
              className="mt-1 w-36 rounded border border-border bg-card px-2 py-1 text-xs"
            />
          </div>
          <button
            type="submit"
            disabled={busy || !path.trim()}
            className="h-8 cursor-pointer rounded border border-border bg-card px-3 text-xs disabled:opacity-50"
          >
            Integrate
          </button>
        </form>
      </section>

      <section aria-labelledby="appimage-install-heading" className="mt-4 border-t border-border pt-3">
        <h3 id="appimage-install-heading" className="text-xs font-semibold">
          Install from a URL
        </h3>
        <form onSubmit={onInstall} className="mt-2 grid gap-2">
          <div>
            <label htmlFor="appimage-url" className="block text-2xs text-muted-foreground">
              HTTPS URL
            </label>
            <input
              id="appimage-url"
              type="url"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              disabled={busy}
              placeholder="https://example.org/App.AppImage"
              className="mt-1 w-full rounded border border-border bg-card px-2 py-1 text-xs"
            />
          </div>
          <div className="flex flex-wrap items-end gap-2">
            <div className="min-w-60 flex-1">
              <label htmlFor="appimage-sha" className="block text-2xs text-muted-foreground">
                SHA-256 (64 hex)
              </label>
              <input
                id="appimage-sha"
                value={sha256}
                onChange={(e) => setSha256(e.target.value)}
                disabled={busy || allowUnverified}
                className="mt-1 w-full rounded border border-border bg-card px-2 py-1 font-mono text-xs"
              />
            </div>
            <div>
              <label htmlFor="appimage-url-name" className="block text-2xs text-muted-foreground">
                Name (optional)
              </label>
              <input
                id="appimage-url-name"
                value={urlName}
                onChange={(e) => setUrlName(e.target.value)}
                disabled={busy}
                className="mt-1 w-32 rounded border border-border bg-card px-2 py-1 text-xs"
              />
            </div>
          </div>
          <div className="flex items-center justify-between gap-2">
            <label className="flex items-center gap-2 text-2xs text-muted-foreground">
              <input
                type="checkbox"
                checked={allowUnverified}
                onChange={(e) => {
                  setAllowUnverified(e.target.checked);
                  if (e.target.checked) setSha256("");
                }}
                disabled={busy}
              />
              Skip checksum (discouraged)
            </label>
            <button
              type="submit"
              disabled={busy || !url.trim() || (!sha256.trim() && !allowUnverified)}
              className="h-8 cursor-pointer rounded border border-border bg-card px-3 text-xs disabled:opacity-50"
            >
              Install
            </button>
          </div>
        </form>
      </section>

      <section aria-labelledby="appimage-list-heading" className="mt-4 border-t border-border pt-3">
        <h3 id="appimage-list-heading" className="text-xs font-semibold">
          Integrated AppImages
        </h3>
        {entries === null ? (
          <p role="status" className="mt-2 text-xs text-muted-foreground">
            Loading…
          </p>
        ) : entries.length === 0 ? (
          <p className="mt-2 text-xs text-muted-foreground">None yet.</p>
        ) : (
          <ul aria-label="Integrated AppImages" className="mt-2 divide-y divide-border">
            {entries.map((entry) => (
              <li key={entry.id} className="flex items-center justify-between gap-3 py-2">
                <div className="min-w-0">
                  <p className="truncate text-xs font-medium">{entry.name}</p>
                  <p className="truncate text-2xs text-muted-foreground" title={entry.sha256}>
                    {entry.id} · {entry.sha256.slice(0, 12)}…
                  </p>
                </div>
                <button
                  type="button"
                  aria-label={`Remove ${entry.name}`}
                  disabled={busy}
                  onClick={() => onRemove(entry)}
                  className="h-8 cursor-pointer rounded border border-border bg-card px-2 text-xs disabled:opacity-50"
                >
                  Remove
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>

      {removeSpec !== null && (
        <ConfirmDialog
          spec={removeSpec}
          onConfirm={confirmRemove}
          onClose={() => setPendingRemove(null)}
        />
      )}
    </div>
  );
}
