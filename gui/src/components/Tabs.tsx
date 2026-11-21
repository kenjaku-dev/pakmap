// Tabs — navigation (Search | Updates | News | AppImages). Proper tablist
// semantics: tabs are the only consumers of Tab/arrow keys here, each panel
// is labelled by its tab, active tab is aria-selected.
import { useCallback } from "react";

export type TabId = "search" | "updates" | "news" | "appimages";

export const TABS: { id: TabId; label: string; key: string }[] = [
  { id: "search", label: "Search", key: "1" },
  { id: "updates", label: "Updates", key: "2" },
  { id: "news", label: "News", key: "3" },
  { id: "appimages", label: "AppImages", key: "4" },
];

export function Tabs({
  active,
  onChange,
}: {
  active: TabId;
  onChange: (tab: TabId) => void;
}) {
  const onKey = useCallback(
    (e: React.KeyboardEvent<HTMLDivElement>) => {
      const idx = TABS.findIndex((t) => t.id === active);
      let next: number | null = null;
      if (e.key === "ArrowRight") next = (idx + 1) % TABS.length;
      else if (e.key === "ArrowLeft") next = (idx - 1 + TABS.length) % TABS.length;
      else if (e.key === "Home") next = 0;
      else if (e.key === "End") next = TABS.length - 1;
      if (next !== null) {
        e.preventDefault();
        e.stopPropagation();
        onChange(TABS[next]!.id);
        document
          .querySelector<HTMLButtonElement>(`#tab-${TABS[next]!.id}`)
          ?.focus();
      }
    },
    [active, onChange],
  );

  return (
    <div
      role="tablist"
      aria-label="Areas"
      onKeyDown={onKey}
      className="flex items-center gap-1 border-b border-border bg-card px-3"
    >
      {TABS.map((t) => (
        <button
          key={t.id}
          id={`tab-${t.id}`}
          role="tab"
          type="button"
          aria-selected={active === t.id}
          aria-controls={`panel-${t.id}`}
          tabIndex={active === t.id ? 0 : -1}
          onClick={() => onChange(t.id)}
          className={`cursor-pointer border-b-2 px-3 py-1.5 text-xs transition-colors ${
            active === t.id
              ? "border-foreground font-semibold text-foreground"
              : "border-transparent text-muted-foreground hover:text-foreground"
          }`}
        >
          {t.label}
        </button>
      ))}
    </div>
  );
}
