import type { Metadata, Viewport } from "next";
import { JetBrains_Mono } from "next/font/google";
import { AuroraBackground } from "@/components/AuroraBackground";
import { MotionProvider } from "@/components/MotionProvider";
import "./globals.css";

const SITE_URL = "https://kenjaku-dev.github.io/pakmap";

const jetbrains = JetBrains_Mono({
  variable: "--font-jetbrains",
  subsets: ["latin"],
  display: "swap",
});

export const metadata: Metadata = {
  metadataBase: new URL(SITE_URL),
  title: "pakmap — Fast AUR + repo search for Arch",
  description:
    "One Rust binary for pacman and AUR search, with a terminal UI to install and remove packages. Built with ratatui and libalpm.",
  keywords: [
    "pakmap",
    "arch linux",
    "aur",
    "pacman",
    "flatpak",
    "flathub",
    "tui",
    "rust",
    "package search",
  ],
  alternates: {
    canonical: "./",
  },
  openGraph: {
    title: "pakmap — Fast AUR + repo search for Arch",
    description:
      "Search official repos, the AUR, and Flatpak; install from a TUI — one binary for Arch/Artix.",
    type: "website",
    // image comes from src/app/opengraph-image.tsx (file convention)
  },
  twitter: {
    card: "summary_large_image",
    title: "pakmap — Fast AUR + repo search for Arch",
    description:
      "Search official repos, the AUR, and Flatpak from a TUI — one binary for Arch/Artix.",
  },
};

export const viewport: Viewport = {
  // both schemes shipped (v0.9.1 §5) — the toolbar/requested scheme follows
  // the OS; themeColor pairs per scheme via the media-query form.
  colorScheme: "dark light",
  themeColor: [
    { media: "(prefers-color-scheme: dark)", color: "#080d17" },
    { media: "(prefers-color-scheme: light)", color: "#f1f5f9" },
  ],
};

export default function RootLayout({ children }: LayoutProps<"/">) {
  return (
    <html lang="en" className={`${jetbrains.variable} h-full antialiased`}>
      <body className="min-h-full flex flex-col">
        <AuroraBackground />
        <MotionProvider>{children}</MotionProvider>
      </body>
    </html>
  );
}
