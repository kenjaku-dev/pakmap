import { describe, expect, it } from "vitest";
import {
  MAX_OP_LINES,
  opLabel,
  opsReducer,
  type OpsState,
} from "./opsReducer";
import type { OpRequest } from "../types";

const initial: OpsState = { op: null };

const req: OpRequest = { kind: "install_repo", args: { name: "htop" } };

function started(id = 1, label = "Install htop"): OpsState {
  return opsReducer(initial, { type: "start", id, label, request: req });
}

describe("opsReducer", () => {
  it("accumulates progress and tracks the phase", () => {
    let s = started();
    s = opsReducer(s, {
      type: "message",
      msg: { kind: "progress", op_id: 1, phase: "install", line: "resolving deps" },
    });
    s = opsReducer(s, {
      type: "message",
      msg: { kind: "progress", op_id: 1, phase: "install", line: "installing htop-3.4" },
    });
    expect(s.op?.status).toBe("running");
    expect(s.op?.phase).toBe("install");
    expect(s.op?.lines).toEqual(["resolving deps", "installing htop-3.4"]);
  });

  it("done is terminal with the classified error", () => {
    let s = started();
    s = opsReducer(s, {
      type: "message",
      msg: {
        kind: "done",
        op_id: 1,
        ok: false,
        exit_code: 255,
        error: { code: "auth_dismissed", message: "auth failed", hint: "retry" },
      },
    });
    expect(s.op?.status).toBe("failed");
    expect(s.op?.exit_code).toBe(255);
    expect(s.op?.error?.code).toBe("auth_dismissed");
  });

  it("success stays successful without an error", () => {
    let s = started();
    s = opsReducer(s, {
      type: "message",
      msg: { kind: "done", op_id: 1, ok: true, exit_code: 0, error: null },
    });
    expect(s.op?.status).toBe("ok");
    expect(s.op?.error).toBeNull();
  });

  it("drops progress and a second done after the terminal event", () => {
    let s = started();
    s = opsReducer(s, {
      type: "message",
      msg: { kind: "done", op_id: 1, ok: true, exit_code: 0, error: null },
    });
    s = opsReducer(s, {
      type: "message",
      msg: { kind: "progress", op_id: 1, phase: "late", line: "straggler" },
    });
    s = opsReducer(s, {
      type: "message",
      msg: { kind: "done", op_id: 1, ok: false, exit_code: 9, error: null },
    });
    expect(s.op?.status).toBe("ok");
    expect(s.op?.phase).toBe("done");
    expect(s.op?.lines).not.toContain("straggler");
  });

  it("ignores messages for other op ids (generation guard)", () => {
    let s = started(1);
    s = opsReducer(s, {
      type: "message",
      msg: { kind: "progress", op_id: 2, phase: "x", line: "alien op" },
    });
    expect(s.op?.lines).toEqual([]);
    expect(s.op?.phase).toBe("prepare");
  });

  it("cancel_sent marks canceling and blocks re-cancel", () => {
    let s = started();
    s = opsReducer(s, { type: "cancel_sent", id: 1 });
    expect(s.op?.status).toBe("canceling");
    s = opsReducer(s, { type: "cancel_sent", id: 1 });
    expect(s.op?.status).toBe("canceling");
  });

  it("cancel_sent for an unknown op id is a no-op", () => {
    const s = opsReducer(started(), { type: "cancel_sent", id: 99 });
    expect(s.op?.status).toBe("running");
  });

  it("dismiss clears terminal ops but never a running one", () => {
    let s = started();
    s = opsReducer(s, { type: "dismiss" });
    expect(s.op?.status).toBe("running"); // protected

    let t = started();
    t = opsReducer(t, {
      type: "message",
      msg: { kind: "done", op_id: 1, ok: true, exit_code: 0, error: null },
    });
    t = opsReducer(t, { type: "dismiss" });
    expect(t.op).toBeNull();
  });

  it("caps the journal ring at MAX_OP_LINES", () => {
    let s = started();
    for (let i = 0; i < MAX_OP_LINES + 20; i++) {
      s = opsReducer(s, {
        type: "message",
        msg: { kind: "progress", op_id: 1, phase: "install", line: `line ${i}` },
      });
    }
    expect(s.op?.lines).toHaveLength(MAX_OP_LINES);
    expect(s.op?.lines?.[0]).toBe(`line ${20}`); // oldest trimmed
    expect(s.op?.lines?.at(-1)).toBe(`line ${MAX_OP_LINES + 19}`);
  });

  it("labels every op request shape", () => {
    expect(opLabel({ kind: "install_repo", args: { name: "htop" } })).toBe("Install htop");
    expect(opLabel({ kind: "install_aur", args: { name: "yay" } })).toBe("Build & install (AUR) yay");
    expect(opLabel({ kind: "remove", args: { name: "vim" } })).toBe("Remove vim");
    expect(opLabel({ kind: "remove_orphans" })).toBe("Remove orphans");
    expect(opLabel({ kind: "upgrade" })).toBe("Full system upgrade");
    expect(opLabel({ kind: "refresh" })).toBe("Refresh databases");
    expect(opLabel({ kind: "clean", args: {} })).toBe("Clean package cache");
    expect(opLabel({ kind: "clean", args: { keep: 3 } })).toBe("Clean package cache");
    expect(opLabel({ kind: "install_flatpak", args: { app_id: "org.Foo" } })).toBe(
      "Install flatpak org.Foo",
    );
    expect(opLabel({ kind: "remove_flatpak", args: { app_id: "org.Foo" } })).toBe(
      "Remove flatpak org.Foo",
    );
  });
});
