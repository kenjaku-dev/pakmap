"use client";

import { ArrowUp } from "lucide-react";

export function BackToTop() {
  function handleClick() {
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    window.scrollTo({ top: 0, behavior: reduce ? "auto" : "smooth" });
    document.getElementById("main")?.focus({ preventScroll: true });
  }

  return (
    <button
      type="button"
      onClick={handleClick}
      className="inline-flex min-h-11 cursor-pointer items-center gap-1.5 rounded-md border border-border px-3 py-1.5 text-xs text-muted-foreground transition-colors duration-200 hover:border-accent/50 hover:text-foreground"
    >
      <ArrowUp aria-hidden="true" className="h-3.5 w-3.5" />
      Back to top
    </button>
  );
}
