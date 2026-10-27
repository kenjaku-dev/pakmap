// v0.9.1 §5: authored OG social card (was the generic GitHub opengraph
// auto-render). Built with next/og ImageResponse at build time — part of
// `out/`, no binary asset to commit. Default font (no network fetch at
// build stays CI-safe); flexbox-only JSX (satori constraint).
import { ImageResponse } from "next/og";

export const alt = "pakmap — fast pacman + AUR + Flatpak search and ops for Arch Linux";
// `output: export` requires this to be a known-static route.
export const dynamic = "force-static";
export const size = {
  width: 1200,
  height: 630,
};
export const contentType = "image/png";

export default function Image() {
  return new ImageResponse(
    (
      <div
        style={{
          height: "100%",
          width: "100%",
          display: "flex",
          flexDirection: "column",
          justifyContent: "space-between",
          background: "#080d17",
          padding: 72,
          position: "relative",
        }}
      >
        {/* accent edge */}
        <div
          style={{
            position: "absolute",
            top: 0,
            left: 0,
            right: 0,
            height: 8,
            background: "#22c55e",
          }}
        />
        <div style={{ display: "flex", flexDirection: "column" }}>
          <div
            style={{
              display: "flex",
              fontSize: 96,
              fontWeight: 700,
              color: "#f1f5f9",
              letterSpacing: "-0.02em",
            }}
          >
            pak<span style={{ color: "#22c55e" }}>map</span>
          </div>
          <div
            style={{
              fontSize: 40,
              color: "#94a3b8",
              marginTop: 20,
              lineHeight: 1.3,
            }}
          >
            One Rust binary for Arch: search repos, the AUR, and Flatpak —
            then install, clean, and keep watch from a TUI.
          </div>
        </div>

        <div style={{ display: "flex", gap: 18 }}>
          {[
            { label: "pacman + AUR + Flatpak", color: "#22c55e" },
            { label: "TUI install / remove", color: "#38bdf8" },
            { label: "updates · cache · stats", color: "#a78bfa" },
          ].map((chip) => (
            <div
              key={chip.label}
              style={{
                display: "flex",
                fontSize: 28,
                color: chip.color,
                border: `2px solid ${chip.color}`, // satori: rgb literals only in borders
                borderColor: `${chip.color}66`,
                borderRadius: 12,
                padding: "10px 22px",
              }}
            >
              {chip.label}
            </div>
          ))}
        </div>

        <div style={{ display: "flex", fontSize: 24, color: "#64748b" }}>
          kenjaku-dev.github.io/pakmap
        </div>
      </div>
    ),
    { ...size },
  );
}
