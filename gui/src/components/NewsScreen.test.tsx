// @vitest-environment jsdom
// v0.9.3 Phase 3 — news-row link actions against the shared open gate:
// blank (source-blanked/untrusted) links disable Open/Copy IN PLACE — the
// buttons stay visible with proper disabled semantics (accessibility: no
// layout shift, state announced), and no open call leaves the frontend.
// jsx: react plugin is configured in vitest.config.ts (automatic runtime).
// Note: no @testing-library/jest-dom here — plain vitest assertions only.
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { NewsScreen } from "./NewsScreen";
import type { NewsItemDto } from "../types";

const openExternal = vi.fn();

vi.mock("../lib/ipc", () => ({
  getNews: vi.fn(),
  normalizeGuiError: (e: unknown) => e,
  openExternal: (...args: unknown[]) => openExternal(...args),
}));

import { getNews } from "../lib/ipc";

const item = (title: string, link: string): NewsItemDto => ({
  title,
  link,
  pub_date: "Mon, 12 Oct 2026 10:00:00 +0000",
  pub_epoch: 1760256000,
  needs_action: false,
});

const fixture: NewsItemDto[] = [
  item("Regular update round", "https://archlinux.org/news/round/"),
  // Hostile link as the Rust news layer would have blanked it.
  item("Suspicious announcement", ""),
];

const buttonFor = (action: "Open" | "Copy link", title: string): HTMLButtonElement =>
  screen.getByRole("button", {
    name: `${action === "Open" ? "Open news item in browser" : "Copy link to news item"}: ${title}`,
  }) as HTMLButtonElement;

describe("NewsScreen disabled link actions", () => {
  beforeEach(() => {
    cleanup();
    openExternal.mockClear();
    vi.mocked(getNews).mockResolvedValue(fixture);
  });

  it("keeps the safe row fully actionable", async () => {
    render(<NewsScreen />);
    await waitFor(() => expect(buttonFor("Open", "Regular update round").disabled).toBe(false));
    expect(buttonFor("Copy link", "Regular update round").disabled).toBe(false);
  });

  it("disables open/copy for a blank link without removing the buttons", async () => {
    render(<NewsScreen />);
    await waitFor(() => expect(buttonFor("Open", "Suspicious announcement").disabled).toBe(true));
    // Still present in the DOM (axe: visible-but-inert beats removal).
    expect(buttonFor("Open", "Suspicious announcement")).toBeTruthy();
    const copy = buttonFor("Copy link", "Suspicious announcement");
    expect(copy.disabled).toBe(true);
    expect(copy).toBeTruthy();
    // The row explains WHY it is inert (not a mystery button).
    expect(screen.getByText(/link untrusted/)).toBeTruthy();
  });

  it("routes a safe open through openExternal with the audit label", async () => {
    render(<NewsScreen />);
    const open = (await screen.findByRole("button", {
      name: "Open news item in browser: Regular update round",
    })) as HTMLButtonElement;
    expect(open.disabled).toBe(false);
    fireEvent.click(open);
    await waitFor(() => expect(openExternal).toHaveBeenCalledTimes(1));
    expect(openExternal).toHaveBeenCalledWith(
      "https://archlinux.org/news/round/",
      "Regular update round",
    );
  });

  it("leaves no open call behind after clicking a disabled button", async () => {
    render(<NewsScreen />);
    await waitFor(() =>
      expect(buttonFor("Copy link", "Suspicious announcement").disabled).toBe(true),
    );
    fireEvent.click(buttonFor("Copy link", "Suspicious announcement"));
    // Native disabled buttons swallow events — assert nothing escaped.
    expect(openExternal).not.toHaveBeenCalled();
  });
});
