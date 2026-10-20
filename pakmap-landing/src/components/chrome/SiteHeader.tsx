"use client";

import { useState } from "react";
import { ExternalLink } from "lucide-react";
import { Logo } from "./Logo";
import { MobileNav } from "./MobileNav";
import { NavLinks } from "./NavLinks";
import { useActiveSection } from "./useActiveSection";
import { useStuckFallback } from "./useStuckFallback";
import { SITE } from "@/lib/site";

export function SiteHeader() {
  const [menuOpen, setMenuOpen] = useState(false);
  const activeHref = useActiveSection();
  const { setRef, stuck } = useStuckFallback<HTMLElement>();

  return (
    <>
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:fixed focus:left-4 focus:top-4 focus:z-[60] focus:rounded-md focus:border focus:border-accent focus:bg-card focus:px-4 focus:py-2 focus:text-sm focus:text-foreground"
      >
        Skip to content
      </a>

      <header
        ref={setRef}
        data-stuck={stuck ? "true" : "false"}
        className="site-header sticky top-0 z-50"
      >
        <div aria-hidden="true" className="scroll-progress" />
        <div className="site-header-bar border-b border-border/60 bg-background/80 backdrop-blur-xl">
          <div className="mx-auto flex h-14 w-full max-w-5xl items-center justify-between gap-3 px-5 sm:px-8">
            <Logo />

            <nav aria-label="Primary" className="hidden sm:block">
              <NavLinks activeHref={activeHref} className="sm:gap-1" />
            </nav>

            <div className="flex items-center gap-2">
              <a
                href={SITE.githubReleases}
                target="_blank"
                rel="noopener noreferrer"
                className="hidden text-xs text-muted-foreground tabular-nums transition-colors duration-200 hover:text-foreground md:inline"
              >
                <span className="sr-only">
                  Version {SITE.version}, release notes (opens in new tab)
                </span>
                <span aria-hidden="true">v{SITE.version}</span>
              </a>
              <a
                href="#install"
                className="hidden min-h-10 cursor-pointer items-center rounded-md bg-accent px-3 py-1.5 text-xs font-semibold text-on-accent transition-opacity duration-200 hover:opacity-90 sm:inline-flex"
              >
                Install
              </a>
              <a
                href={SITE.github}
                target="_blank"
                rel="noopener noreferrer"
                aria-label="Source on GitHub (opens in new tab)"
                className="inline-flex min-h-10 cursor-pointer items-center gap-1.5 rounded-md border border-border px-3 py-1.5 text-xs text-muted-foreground transition-colors duration-200 hover:border-accent/50 hover:text-foreground"
              >
                GitHub
                <ExternalLink aria-hidden="true" className="h-3 w-3" />
              </a>
              <MobileNav
                open={menuOpen}
                onOpenChange={setMenuOpen}
                activeHref={activeHref}
              />
            </div>
          </div>
        </div>
      </header>
    </>
  );
}
