"use client";

import { motion, useReducedMotion } from "framer-motion";
import { ArrowDown, ChevronDown } from "lucide-react";
import { TerminalDemo } from "./TerminalDemo";
import { CopyButton } from "./ui/CopyButton";
import { Kbd } from "./ui/Kbd";
import { INSTALL_CMD } from "@/lib/site";

function GitHubIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="currentColor"
      aria-hidden="true"
      className={className}
    >
      <path d="M12 2C6.477 2 2 6.486 2 12.017c0 4.425 2.865 8.18 6.839 9.504.5.092.682-.217.682-.483 0-.237-.009-.868-.014-1.703-2.782.605-3.369-1.343-3.369-1.343-.454-1.158-1.11-1.466-1.11-1.466-.908-.62.069-.608.069-.608 1.003.07 1.531 1.032 1.531 1.032.892 1.53 2.341 1.088 2.91.832.092-.647.35-1.088.636-1.338-2.22-.253-4.555-1.113-4.555-4.951 0-1.093.39-1.988 1.029-2.688-.103-.253-.446-1.272.098-2.65 0 0 .84-.27 2.75 1.026A9.564 9.564 0 0 1 12 6.844a9.59 9.59 0 0 1 2.504.337c1.909-1.296 2.747-1.027 2.747-1.027.546 1.379.202 2.398.1 2.651.64.7 1.028 1.595 1.028 2.688 0 3.848-2.339 4.695-4.566 4.943.359.309.678.92.678 1.855 0 1.338-.012 2.419-.012 2.747 0 .268.18.58.688.482A10.02 10.02 0 0 0 22 12.017C22 6.486 17.523 2 12 2Z" />
    </svg>
  );
}

const SHORTCUTS = [
  { keys: ["/"], label: "search" },
  { keys: ["j", "k"], label: "move" },
  { keys: ["Enter"], label: "install" },
  { keys: ["i"], label: "info" },
  { keys: ["Tab"], label: "remove" },
  { keys: ["q"], label: "quit" },
] as const;

export function Hero() {
  const reduce = useReducedMotion();
  const ease = [0.22, 1, 0.36, 1] as const;

  return (
    <section id="top" className="relative overflow-hidden">
      <div className="relative mx-auto grid w-full max-w-5xl items-center gap-10 px-5 pb-20 pt-16 sm:px-8 sm:pt-20 lg:min-h-[calc(100svh-3.5rem)] lg:grid-cols-[1fr_1.05fr] lg:gap-12 lg:pt-14">
        <div className="max-w-xl">
          <motion.p
            initial={reduce ? false : { opacity: 0, y: 8 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.35, ease }}
            className="mb-5 inline-flex items-center gap-2 rounded-full border border-accent/25 bg-accent/5 px-3 py-1 text-[11px] text-muted-foreground backdrop-blur-sm"
          >
            <span className="relative flex h-1.5 w-1.5" aria-hidden="true">
              <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-accent opacity-60" />
              <span className="relative inline-flex h-1.5 w-1.5 rounded-full bg-accent" />
            </span>
            Rust · ratatui · libalpm · Tauri · MIT
          </motion.p>

          <motion.h1
            initial={reduce ? false : { opacity: 0, y: 12 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.45, ease, delay: 0.05 }}
            className="text-balance text-[clamp(1.9rem,3.1vw+0.55rem,2.9rem)] font-semibold leading-[1.12] tracking-tight text-foreground"
          >
            Search Arch packages
            <span className="text-gradient block">at terminal speed.</span>
          </motion.h1>

          <motion.p
            initial={reduce ? false : { opacity: 0, y: 12 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.45, ease, delay: 0.12 }}
            className="mt-5 max-w-[42ch] text-sm leading-relaxed text-muted-foreground sm:text-base"
          >
            One binary for <span className="text-foreground">pacman -Ss</span>,
            AUR RPC v5, and flatpak search. Search in parallel, preview the
            PKGBUILD, install or remove — without leaving your terminal.
          </motion.p>

          <motion.div
            initial={reduce ? false : { opacity: 0, y: 12 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.45, ease, delay: 0.18 }}
            className="mt-7 flex flex-wrap items-center gap-3"
          >
            <a
              href="#install"
              className="btn-primary-glow inline-flex cursor-pointer items-center gap-2 rounded-md bg-accent px-4 py-2.5 text-sm font-semibold text-on-accent"
            >
              Get pakmap
              <ArrowDown aria-hidden="true" className="h-4 w-4" />
            </a>
            <a
              href="https://github.com/kenjaku-dev/pakmap"
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex cursor-pointer items-center gap-2 rounded-md border border-border bg-card/60 px-4 py-2.5 text-sm text-muted-foreground backdrop-blur-sm transition-colors duration-200 hover:border-accent/50 hover:text-foreground"
            >
              <GitHubIcon className="h-4 w-4" />
              Source
            </a>
          </motion.div>

          <motion.div
            initial={reduce ? false : { opacity: 0, y: 12 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.45, ease, delay: 0.24 }}
            className="mt-5 flex flex-wrap items-center gap-2 text-xs text-muted-foreground"
          >
            <code className="break-all rounded-md border border-border bg-card/80 px-2.5 py-1.5 text-[11px] text-accent-secondary backdrop-blur-sm sm:text-xs">
              {INSTALL_CMD}
            </code>
            <CopyButton text={INSTALL_CMD} label="Copy install command" />
          </motion.div>

          <motion.dl
            initial={reduce ? false : { opacity: 0, y: 12 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.45, ease, delay: 0.3 }}
            className="mt-7 flex flex-wrap items-center gap-x-4 gap-y-2 border-t border-border/40 pt-5"
          >
            {SHORTCUTS.map((shortcut) => (
              <div key={shortcut.label} className="flex items-center gap-1.5">
                <dt className="sr-only">{shortcut.label}</dt>
                <dd className="flex items-center gap-1" aria-hidden="true">
                  {shortcut.keys.map((key) => (
                    <Kbd key={key}>{key}</Kbd>
                  ))}
                </dd>
                <dd aria-hidden="true" className="text-[11px] text-muted-foreground">
                  {shortcut.label}
                </dd>
              </div>
            ))}
          </motion.dl>
        </div>

        <motion.div
          initial={reduce ? false : { opacity: 0, y: 24, rotateX: 6 }}
          animate={{ opacity: 1, y: 0, rotateX: 0 }}
          transition={{ duration: 0.6, ease, delay: 0.15 }}
          style={{ transformPerspective: 1200 }}
          className="relative w-full"
        >
          <div
            aria-hidden="true"
            className="absolute -inset-8 rounded-[2rem] bg-[radial-gradient(ellipse_60%_60%_at_50%_40%,color-mix(in_srgb,var(--accent)_14%,transparent),transparent)] blur-2xl"
          />
          <div className="hero-terminal relative rounded-xl bg-gradient-to-b from-accent/40 via-border/50 to-border/20 p-px shadow-[0_24px_70px_-20px_rgba(0,0,0,0.7)]">
            <TerminalDemo />
          </div>
        </motion.div>
      </div>

      <motion.a
        href="#features"
        aria-label="Scroll to features"
        initial={reduce ? false : { opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ delay: 0.9, duration: 0.5 }}
        className="group absolute inset-x-0 bottom-5 mx-auto hidden w-max cursor-pointer flex-col items-center gap-1.5 text-muted-foreground transition-colors duration-200 hover:text-foreground sm:flex"
      >
        <span className="text-[10px] uppercase tracking-[0.25em]">scroll</span>
        <motion.span
          animate={reduce ? undefined : { y: [0, 5, 0] }}
          transition={{ duration: 1.6, repeat: Infinity, ease: "easeInOut" }}
        >
          <ChevronDown aria-hidden="true" className="h-4 w-4" />
        </motion.span>
      </motion.a>
    </section>
  );
}

