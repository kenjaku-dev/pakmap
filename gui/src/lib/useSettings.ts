// useSettings — the settings screen's data controller (v0.10.0 phase 2, plan
// §3). One whole-catalog read from the engine (`get_effective_config`), plus
// per-key writes through the SAME engine doors the CLI uses. State stays
// local and patchy (origin flips on a successful write like the engine's
// origins map would show on refetch); after any success the screen calls
// onChanged() so App re-arms the operational flags (readonly/flatpak) and the
// search summary (debounce/limit) for the rest of the UI.
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  getEffectiveConfig,
  setConfigEntry,
  unsetConfigEntry,
} from "./ipc";
import type {
  EffectiveConfig,
  GuiError,
  SettingsEntry,
  SettingsTarget,
} from "../types";

export type RowError = {
  message: string;
  hint: string | null;
  /** engine refusal was "config changed on disk" → offer a reload banner */
  conflict: boolean;
  /** policy-refused about a missing USER file → offer "Create config" */
  canCreateUser: boolean;
};

export type SettingsTargetState = SettingsTarget;

export const WRITE_TARGET_LABELS: Record<SettingsTargetState, string> = {
  user: "User config (~/.config/pakmap/config.toml)",
  project: "Project config (./pakmap.toml)",
};

const isMissingUserFileRefusal = (message: string): boolean =>
  message.includes("does not exist") && !message.includes("project");

export function useSettings(onChanged?: () => void) {
  const [data, setData] = useState<EffectiveConfig | null>(null);
  const [loadError, setLoadError] = useState<GuiError | null>(null);
  const [query, setQuery] = useState("");
  const [groupFilter, setGroupFilter] = useState<string>("All");
  const [target, setTarget] = useState<SettingsTargetState>("user");
  const [rowErrors, setRowErrors] = useState<Record<string, RowError>>({});
  const [conflict, setConflict] = useState(false);
  /** in-flight op map so a row's control can render busy without freezing */
  const [busyKey, setBusyKey] = useState<string | null>(null);

  const reload = useCallback(() => {
    getEffectiveConfig()
      .then((d) => {
        setData(d);
        setRowErrors({});
        setConflict(false);
        setTarget((t) => (t === "project" && d.project_path === null ? "user" : t));
      })
      .catch((e: GuiError) => setLoadError(e));
  }, []);

  useEffect(() => {
    reload();
  }, [reload]);

  const clearRowError = useCallback((key: string) => {
    setRowErrors((prev) => {
      if (!(key in prev)) return prev;
      const next = { ...prev };
      delete next[key];
      return next;
    });
  }, []);

  const failRow = useCallback((key: string, e: GuiError) => {
    setRowErrors((prev) => ({
      ...prev,
      [key]: {
        message: e.message,
        hint: e.hint,
        conflict: e.code === "conflict",
        canCreateUser:
          e.code === "policy-refused" && isMissingUserFileRefusal(e.message),
      },
    }));
    setConflict((c) => c || e.code === "conflict");
  }, []);

  const set = useCallback(
    (key: string, value: string, opts?: { create?: boolean }) => {
      setBusyKey(key);
      clearRowError(key);
      setConfigEntry(key, value, target, opts?.create ?? false)
        .then(() => {
          setBusyKey(null);
          // patch locally, then let the rest of the app re-arm flags/summary
          setData((d) =>
            d === null
              ? d
              : {
                  ...d,
                  entries: d.entries.map((e) =>
                    e.key === key
                      ? {
                          ...e,
                          value,
                          origin: {
                            label: target === "user" ? "user" : "project",
                            path:
                              target === "user"
                                ? (d.user_path ?? null)
                                : (d.project_path ?? null),
                          },
                          valid: true,
                        }
                      : e,
                  ),
                },
          );
          onChanged?.();
        })
        .catch((e: GuiError) => {
          setBusyKey(null);
          failRow(key, e);
        });
    },
    [clearRowError, failRow, onChanged, target],
  );

  const unset = useCallback(
    (key: string) => {
      setBusyKey(key);
      clearRowError(key);
      unsetConfigEntry(key, target)
        .then(() => {
          setBusyKey(null);
          setData((d) =>
            d === null
              ? d
              : {
                  ...d,
                  entries: d.entries.map((e) =>
                    e.key === key
                      ? {
                          ...e,
                          value: e.default,
                          origin: { label: "default", path: null },
                          valid: true,
                        }
                      : e,
                  ),
                },
          );
          onChanged?.();
        })
        .catch((e: GuiError) => {
          setBusyKey(null);
          failRow(key, e);
        });
    },
    [clearRowError, failRow, onChanged, target],
  );

  const groups = useMemo(() => {
    if (data === null) return [];
    const seen: string[] = [];
    for (const e of data.entries) {
      if (seen.includes(e.group)) continue;
      seen.push(e.group);
    }
    return ["All", ...seen, "Environment"];
  }, [data]);

  const visible = useMemo(() => {
    if (data === null) return [];
    const q = query.trim().toLowerCase();
    return data.entries.filter((e) => {
      if (groupFilter !== "All" && e.group !== groupFilter) return false;
      if (q === "") return true;
      return (
        e.key.toLowerCase().includes(q) ||
        e.desc.toLowerCase().includes(q) ||
        e.group.toLowerCase().includes(q)
      );
    });
  }, [data, groupFilter, query]);

  return {
    data,
    loadError,
    query,
    setQuery,
    groupFilter,
    setGroupFilter,
    groups,
    visible,
    target,
    setTarget,
    rowErrors,
    conflict,
    reload,
    set,
    unset,
    busyKey,
    clearRowError,
  };
}

export type SettingsController = ReturnType<typeof useSettings>;

/** change detection: a row counts as customized when origin ≠ default */
export function isCustomized(e: SettingsEntry): boolean {
  return e.origin.label !== "default";
}
