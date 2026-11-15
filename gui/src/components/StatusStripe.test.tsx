// @vitest-environment jsdom
// v0.10.3 deferred phase — the launch-maintenance note rides the boot
// status line (TUI parity: `{status} — {note}`), transient by contract
// (App clears it on the first search; this component only renders it).
// jsx: react plugin is configured in vitest.config.ts (automatic runtime).
// Note: no @testing-library/jest-dom here — plain vitest assertions only.
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { StatusStripe } from "./StatusStripe";
import { initialSearchState } from "../state/searchReducer";

afterEach(cleanup);

describe("StatusStripe maintenance note", () => {
  it("appends the note to the boot status line", () => {
    render(
      <StatusStripe
        state={initialSearchState}
        maintenanceNote="Pruned 1 AUR clone dirs (512 B)"
      />,
    );
    const text = screen.getByRole("status").textContent ?? "";
    expect(text).toContain(" — Pruned 1 AUR clone dirs (512 B)");
  });

  it("leaves the status line untouched without a note", () => {
    render(<StatusStripe state={initialSearchState} />);
    const text = screen.getByRole("status").textContent ?? "";
    expect(text).not.toContain("Pruned");
  });
});
