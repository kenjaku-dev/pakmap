// @vitest-environment jsdom
// AppImageScreen contract: forms route through the ipc.ts facade, checksum is
// required by default, and remove actually calls the backend with the id.
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppImageScreen } from "./AppImageScreen";
import type { AppImageEntry } from "../types";

const getAppImageEntries = vi.fn<() => Promise<AppImageEntry[]>>();
const integrateAppImage = vi.fn<(path: string, name: string | null) => Promise<AppImageEntry>>();
const installAppImage = vi.fn<
  (url: string, sha256: string | null, allowUnverified: boolean, name: string | null) => Promise<AppImageEntry>
>();
const removeAppImage = vi.fn<(id: string) => Promise<void>>();

vi.mock("../lib/ipc", () => ({
  getAppImageEntries: () => getAppImageEntries(),
  integrateAppImage: (p: string, n: string | null) => integrateAppImage(p, n),
  installAppImage: (u: string, s: string | null, a: boolean, n: string | null) => installAppImage(u, s, a, n),
  removeAppImage: (id: string) => removeAppImage(id),
  normalizeGuiError: (e: unknown) => e,
}));

const entry: AppImageEntry = {
  id: "firefox-browser",
  name: "Firefox Browser",
  sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
  appimage: "firefox-browser.AppImage",
  desktop: "applications/pakmap-firefox-browser.desktop",
  icon: null,
  source_url: null,
  installed_at: 1,
};

describe("AppImageScreen", () => {
  beforeEach(() => {
    cleanup();
    vi.clearAllMocks();
    getAppImageEntries.mockResolvedValue([entry]);
    integrateAppImage.mockResolvedValue(entry);
    installAppImage.mockResolvedValue(entry);
    removeAppImage.mockResolvedValue(undefined);
  });

  it("renders the integrated list", async () => {
    render(<AppImageScreen />);
    await waitFor(() => expect(screen.getByText("Firefox Browser")).toBeTruthy());
    expect(screen.getByRole("list", { name: "Integrated AppImages" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Remove Firefox Browser" })).toBeTruthy();
  });

  it("integrates a local path through the facade", async () => {
    render(<AppImageScreen />);
    await waitFor(() => expect(screen.getByText("Firefox Browser")).toBeTruthy());
    fireEvent.change(screen.getByLabelText("AppImage path"), {
      target: { value: "/tmp/App.AppImage" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Integrate" }));
    await waitFor(() => expect(integrateAppImage).toHaveBeenCalledWith("/tmp/App.AppImage", null));
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("Integrated"));
  });

  it("requires a checksum unless the opt-out is checked", async () => {
    render(<AppImageScreen />);
    await waitFor(() => expect(screen.getByText("Firefox Browser")).toBeTruthy());
    fireEvent.change(screen.getByLabelText("HTTPS URL"), {
      target: { value: "https://example.org/App.AppImage" },
    });
    expect((screen.getByRole("button", { name: "Install" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.change(screen.getByLabelText("SHA-256 (64 hex)"), {
      target: { value: "ab".repeat(32) },
    });
    expect((screen.getByRole("button", { name: "Install" }) as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Install" }));
    await waitFor(() =>
      expect(installAppImage).toHaveBeenCalledWith(
        "https://example.org/App.AppImage",
        "ab".repeat(32),
        false,
        null,
      ),
    );
  });

  it("removes by id", async () => {
    render(<AppImageScreen />);
    await waitFor(() => expect(screen.getByText("Firefox Browser")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "Remove Firefox Browser" }));
    await waitFor(() => expect(removeAppImage).toHaveBeenCalledWith("firefox-browser"));
  });
});
