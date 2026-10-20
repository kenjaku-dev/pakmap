"use client";

import { useEffect, useRef } from "react";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import { ExternalLink, Menu, X } from "lucide-react";
import { NavLinks } from "./NavLinks";
import { SITE } from "@/lib/site";

type MobileNavProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  activeHref?: string;
};

export function MobileNav({ open, onOpenChange, activeHref }: MobileNavProps) {
  const reduce = useReducedMotion();
  const panelRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;

    // Lock the page behind the open panel; restored on close/unmount.
    const prevOverflow = document.documentElement.style.overflow;
    document.documentElement.style.overflow = "hidden";

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onOpenChange(false);
        triggerRef.current?.focus();
        return;
      }

      if (e.key !== "Tab" || !panelRef.current) return;

      // Cycle includes the trigger — its icon is the X while open, so the
      // close affordance stays keyboard-reachable.
      const focusables: HTMLElement[] = [
        ...panelRef.current.querySelectorAll<HTMLElement>(
          "a[href], button:not([disabled])",
        ),
      ];
      if (triggerRef.current) focusables.push(triggerRef.current);
      if (focusables.length === 0) return;

      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      const active = document.activeElement;

      if (e.shiftKey && active === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && active === last) {
        e.preventDefault();
        first.focus();
      }
    };

    document.addEventListener("keydown", onKeyDown);
    const timer = window.setTimeout(() => {
      const firstLink = panelRef.current?.querySelector<HTMLElement>("a[href]");
      firstLink?.focus();
    }, 50);

    return () => {
      document.removeEventListener("keydown", onKeyDown);
      window.clearTimeout(timer);
      document.documentElement.style.overflow = prevOverflow;
    };
  }, [open, onOpenChange]);

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        id="nav-menu-trigger"
        aria-label={open ? "Close menu" : "Open menu"}
        aria-expanded={open}
        aria-controls="mobile-nav-panel"
        onClick={() => onOpenChange(!open)}
        className="inline-flex h-11 w-11 cursor-pointer items-center justify-center rounded-md border border-border text-muted-foreground transition-colors duration-200 hover:border-accent/50 hover:text-foreground sm:hidden"
      >
        {open ? (
          <X aria-hidden="true" className="h-5 w-5" />
        ) : (
          <Menu aria-hidden="true" className="h-5 w-5" />
        )}
      </button>

      <AnimatePresence>
        {open && (
          <>
            <motion.div
              key="backdrop"
              initial={reduce ? { opacity: 1 } : { opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={reduce ? { opacity: 0 } : { opacity: 0 }}
              transition={{ duration: 0.2 }}
              className="fixed inset-0 z-40 bg-background/70 sm:hidden"
              onClick={() => {
                onOpenChange(false);
                triggerRef.current?.focus();
              }}
              aria-hidden="true"
            />
            <motion.div
              key="panel"
              ref={panelRef}
              id="mobile-nav-panel"
              role="dialog"
              aria-modal="true"
              aria-label="Site menu"
              initial={reduce ? { opacity: 1 } : { opacity: 0, y: -8 }}
              animate={{ opacity: 1, y: 0 }}
              exit={reduce ? { opacity: 0 } : { opacity: 0, y: -8 }}
              transition={{ duration: 0.2, ease: [0.22, 1, 0.36, 1] }}
              className="fixed inset-x-0 top-14 z-50 border-b border-border bg-background px-5 pb-6 pt-3 sm:hidden"
            >
              <NavLinks
                activeHref={activeHref}
                onNavigate={() => onOpenChange(false)}
                className="gap-0"
              />
              <a
                href="#install"
                onClick={() => onOpenChange(false)}
                className="mt-3 flex min-h-11 w-full items-center justify-center rounded-md bg-accent px-4 text-sm font-semibold text-on-accent transition-opacity duration-200 hover:opacity-90"
              >
                Install {SITE.name}
              </a>
              <a
                href={SITE.github}
                target="_blank"
                rel="noopener noreferrer"
                onClick={() => onOpenChange(false)}
                className="mt-3 flex min-h-11 w-full items-center justify-center gap-1.5 rounded-md border border-border px-4 text-sm text-muted-foreground transition-colors duration-200 hover:border-accent/50 hover:text-foreground"
              >
                Source on GitHub
                <span className="sr-only">(opens in new tab)</span>
                <ExternalLink aria-hidden="true" className="h-3.5 w-3.5" />
              </a>
            </motion.div>
          </>
        )}
      </AnimatePresence>
    </>
  );
}
