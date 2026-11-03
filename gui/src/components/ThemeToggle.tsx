// ThemeToggle — three-way segmented control (system / light / dark), radiogroup
// semantics with roving focus, aria-checked state, and system-follow spelled
// out on the system option (usability: the default needs a clear name).
import { Monitor, Moon, Sun } from "lucide-react";
import { useCallback, useRef } from "react";
import type { ThemeMode } from "../lib/useTheme";

const OPTIONS: { mode: ThemeMode; label: string }[] = [
  { mode: "system", label: "Follow system theme" },
  { mode: "light", label: "Light theme" },
  { mode: "dark", label: "Dark theme" },
];

export function ThemeToggle({
  mode,
  onChange,
  locked = false,
  lockedReason,
}: {
  mode: ThemeMode;
  onChange: (mode: ThemeMode) => void;
  /** D1: [gui] theme config override owns the palette — buttons render inert */
  locked?: boolean;
  /** sr/screen-tip copy explaining the lock (origin chip lives in settings) */
  lockedReason?: string;
}) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);

  const onKey = useCallback(
    (e: React.KeyboardEvent, idx: number) => {
      if (locked) return;
      let next: number | null = null;
      if (e.key === "ArrowRight" || e.key === "ArrowDown")
        next = (idx + 1) % OPTIONS.length;
      else if (e.key === "ArrowLeft" || e.key === "ArrowUp")
        next = (idx - 1 + OPTIONS.length) % OPTIONS.length;
      if (next !== null) {
        e.preventDefault();
        e.stopPropagation();
        onChange(OPTIONS[next]!.mode);
        refs.current[next]?.focus();
      }
    },
    [locked, onChange],
  );

  return (
    <div
      role="radiogroup"
      aria-label="Theme"
      className="flex items-center rounded-lg border border-border bg-card-muted p-0.5"
      aria-describedby={locked ? "theme-lock-reason" : undefined}
    >
      {locked && lockedReason !== undefined ? (
        <span id="theme-lock-reason" className="sr-only">{lockedReason}</span>
      ) : null}
      {OPTIONS.map((opt, i) => {
        const checked = mode === opt.mode;
        return (
          <button
            key={opt.mode}
            ref={(el) => {
              refs.current[i] = el;
            }}
            type="button"
            role="radio"
            aria-checked={checked}
            tabIndex={checked ? 0 : -1}
            aria-label={opt.label}
            onClick={() => !locked && onChange(opt.mode)}
            onKeyDown={(e) => onKey(e, i)}
            disabled={locked}
            className={`cursor-pointer rounded-md px-1.5 py-1 text-muted-foreground transition-colors hover:text-foreground ${
              checked ? "bg-card text-foreground" : ""
            } ${locked ? "cursor-not-allowed opacity-50 hover:text-muted-foreground" : ""}`}
          >
            {opt.mode === "system" && <Monitor size={13} aria-hidden="true" />}
            {opt.mode === "light" && <Sun size={13} aria-hidden="true" />}
            {opt.mode === "dark" && <Moon size={13} aria-hidden="true" />}
          </button>
        );
      })}
    </div>
  );
}
