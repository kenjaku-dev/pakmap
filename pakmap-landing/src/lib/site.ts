import { PAKMAP_VERSION } from "./version";

// Canonical one-liner, rendered in the hero and the footer conversion band.
export const INSTALL_CMD =
  "cargo install --git https://github.com/kenjaku-dev/pakmap --locked pakmap";

const GITHUB_URL = "https://github.com/kenjaku-dev/pakmap";

export const SITE = {
  name: "pakmap",
  // Single source of truth: ../Cargo.toml → src/lib/version.ts (generated).
  // Regenerate with `pnpm sync-version`; commit the file with version bumps.
  version: PAKMAP_VERSION,
  license: "MIT",
  platforms: "Arch / Artix",
  tagline: "Search Arch packages at terminal speed.",
  github: GITHUB_URL,
  githubReadme: `${GITHUB_URL}#readme`,
  githubIssues: `${GITHUB_URL}/issues`,
  githubReleases: `${GITHUB_URL}/releases`,
  year: 2026,
} as const;

export const NAV_ITEMS = [
  { href: "#features", label: "Features" },
  { href: "#how", label: "How it works" },
  { href: "#install", label: "Install" },
] as const;

export const FOOTER_COLUMNS = [
  {
    title: "Product",
    links: [
      { href: "#features", label: "Features" },
      { href: "#how", label: "How it works" },
      { href: "#install", label: "Install" },
    ],
  },
  {
    title: "Resources",
    links: [
      { href: SITE.githubReadme, label: "Documentation", external: true },
      { href: SITE.github, label: "Source code", external: true },
      { href: SITE.githubReleases, label: "Releases", external: true },
      { href: SITE.githubIssues, label: "Issues", external: true },
    ],
  },
] as const;
