"use client";

import { motion, useReducedMotion } from "framer-motion";
import {
  AppWindow,
  ChartPie,
  Gauge,
  Layers,
  PackageCheck,
  Paintbrush,
  ShieldCheck,
  SlidersHorizontal,
  Smartphone,
} from "lucide-react";
import { Section } from "./ui/Section";
import { useSpotlight } from "./ui/useSpotlight";

const FEATURES = [
  {
    icon: Gauge,
    title: "One-shot CLI",
    description:
      "pakmap firefox runs repo search first, AUR after — parallel tokio, libalpm local DB. Pipe with --json, filter with --regex and --limit.",
    wide: true,
  },
  {
    icon: Layers,
    title: "TUI that matches your drawing",
    description:
      "Search bar, virtualized results, Enter to install, i for info, ? for help.",
    wide: false,
  },
  {
    icon: PackageCheck,
    title: "Install and remove",
    description:
      "Tab switches to Installed mode — filter, then Enter/d/x to remove with a confirm popup.",
    wide: false,
  },
  {
    icon: ShieldCheck,
    title: "AUR safety built in",
    description:
      "PKGBUILD shown before makepkg, namcap when available, makepkg never runs as root. HTTPS sources only.",
    wide: true,
  },
  {
    icon: Paintbrush,
    title: "Themeable config",
    description:
      "Colors, borders, layout, timeouts in ~/.config/pakmap/config.toml — no recompile.",
    wide: false,
  },
  {
    icon: SlidersHorizontal,
    title: "Script-friendly flags",
    description:
      "--source, --by, --installed-only, --bottom-up, --no-tui, --remove PKG.",
    wide: false,
  },
  {
    icon: Smartphone,
    title: "Flatpak support",
    description:
      "--source flatpak searches Flathub; install and remove app-IDs without sudo. --clean also drops unused runtimes.",
    wide: false,
  },
  {
    icon: AppWindow,
    title: "Desktop GUI",
    description:
      "Same staged search, updates, orphans, and cleanup in a native window (Tauri + webkit2gtk). Unprivileged by default — pkexec for pacman flows.",
    wide: true,
  },
  {
    icon: ChartPie,
    title: "Stats & orphans at a glance",
    description:
      "--orphans (-Qdt), --stats across explicit/deps/updates/cache/flatpak — both --json-capable for your scripts.",
    wide: false,
  },
] as const;

const list = {
  hidden: {},
  show: { transition: { staggerChildren: 0.06 } },
};

const item = {
  hidden: { opacity: 0, y: 16, scale: 0.97 },
  show: {
    opacity: 1,
    y: 0,
    scale: 1,
    transition: { duration: 0.4, ease: [0.34, 1.4, 0.64, 1] as const },
  },
};

type Feature = (typeof FEATURES)[number];

function FeatureCard({ feature }: { feature: Feature }) {
  const { ref, onPointerMove } = useSpotlight<HTMLLIElement>();
  const Icon = feature.icon;

  return (
    <motion.li
      ref={ref}
      onPointerMove={onPointerMove}
      variants={item}
      className={`spotlight-card group overflow-hidden rounded-xl border border-border/60 bg-card/80 p-5 backdrop-blur-sm transition-colors duration-300 hover:border-accent/40 ${
        feature.wide ? "sm:col-span-2" : ""
      }`}
    >
      <div className="mb-3 flex h-9 w-9 items-center justify-center rounded-lg border border-accent/25 bg-gradient-to-br from-accent/15 to-accent-secondary/10 text-accent transition-colors duration-300 group-hover:border-accent/50">
        <Icon aria-hidden="true" className="h-4.5 w-4.5" />
      </div>
      <h3 className="text-sm font-semibold text-foreground">{feature.title}</h3>
      <p className="mt-2 text-[13px] leading-relaxed text-muted-foreground">
        {feature.description}
      </p>
    </motion.li>
  );
}

export function Features() {
  const reduce = useReducedMotion();

  return (
    <Section
      id="features"
      labelledBy="features-heading"
      className="mx-auto w-full max-w-5xl px-5 py-16 sm:px-8 sm:py-24"
    >
      <div className="mb-10 max-w-2xl">
        <p className="mb-3 text-[11px] font-medium uppercase tracking-[0.25em] text-accent">
          Features
        </p>
        <h2
          id="features-heading"
          className="text-balance text-2xl font-semibold tracking-tight text-foreground sm:text-3xl"
        >
          Everything in one binary
        </h2>
        <p className="mt-3 text-sm leading-relaxed text-muted-foreground sm:text-base">
          Search, inspect, install, remove. No helper daemons, no root TUI —
          sudo only when pacman needs it.
        </p>
      </div>

      <motion.ul
        initial={reduce ? false : "hidden"}
        whileInView="show"
        viewport={{ once: true, margin: "-60px" }}
        variants={list}
        className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3"
      >
        {FEATURES.map((feature) => (
          <FeatureCard key={feature.title} feature={feature} />
        ))}
      </motion.ul>
    </Section>
  );
}
