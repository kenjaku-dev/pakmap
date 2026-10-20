import { ExternalLink } from "lucide-react";
import { BackToTop } from "./chrome/BackToTop";
import { Logo } from "./chrome/Logo";
import { CopyButton } from "./ui/CopyButton";
import { FOOTER_COLUMNS, INSTALL_CMD, SITE } from "@/lib/site";

export function Footer() {
  return (
    <footer className="relative border-t border-border/60">
      <div
        aria-hidden="true"
        className="absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-accent/50 to-transparent"
      />
      <div className="mx-auto w-full max-w-5xl px-5 pb-8 pt-12 sm:px-8">
        <div className="grid grid-cols-1 gap-10 sm:grid-cols-[1.4fr_1fr_1fr]">
          <div className="max-w-sm">
            <div className="mb-3">
              <Logo />
            </div>
            <p className="text-sm leading-relaxed text-muted-foreground">
              Fast package search for Arch and Artix — official repos, the AUR,
              and Flatpak in one Rust binary, with a terminal UI and a desktop
              GUI to install, remove, and clean.
            </p>
            <p className="mt-4 text-xs text-muted-foreground">
              Built with Rust, ratatui, libalpm, and Tauri.
            </p>
          </div>

          {FOOTER_COLUMNS.map((column) => (
            <nav key={column.title} aria-label={column.title}>
              <h2 className="mb-3 text-xs font-semibold text-foreground">
                {column.title}
              </h2>
              <ul className="flex list-none flex-col gap-2">
                {column.links.map((link) => (
                  <li key={link.href}>
                    <a
                      href={link.href}
                      {...("external" in link && link.external
                        ? { target: "_blank", rel: "noopener noreferrer" }
                        : {})}
                      className="inline-flex min-h-8 items-center text-sm text-muted-foreground transition-colors duration-200 hover:text-foreground"
                    >
                      {link.label}
                    </a>
                  </li>
                ))}
              </ul>
            </nav>
          ))}
        </div>

        {/* Mega-Footer Conversion — the close-out CTA (ui-patterns): the
            canonical one-liner, one copy affordance, one outbound link. */}
        <div className="mt-10 flex flex-col gap-4 rounded-xl border border-border bg-card/50 p-5 sm:flex-row sm:items-center sm:justify-between sm:p-6">
          <div className="min-w-0">
            <p className="text-[11px] font-semibold uppercase tracking-[0.22em] text-accent">
              Get pakmap
            </p>
            <p className="mt-2 max-w-md text-sm leading-relaxed text-muted-foreground">
              Terminal UI for the daily flow, desktop GUI when you want a
              window.
            </p>
            <code className="mt-3 block break-all rounded-md border border-border bg-background/80 px-2.5 py-1.5 text-[11px] text-accent-secondary backdrop-blur-sm sm:text-xs">
              {INSTALL_CMD}
            </code>
          </div>
          <div className="flex flex-shrink-0 flex-wrap items-center gap-2">
            <CopyButton text={INSTALL_CMD} label="Copy install command" />
            <a
              href={SITE.githubReleases}
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex min-h-9 cursor-pointer items-center gap-1.5 rounded-md border border-border px-3 py-1.5 text-xs text-muted-foreground transition-colors duration-200 hover:border-accent/50 hover:text-foreground"
            >
              Releases
              <span className="sr-only">(opens in new tab)</span>
              <ExternalLink aria-hidden="true" className="h-3 w-3" />
            </a>
          </div>
        </div>

        <div className="mt-10 flex flex-col gap-4 border-t border-border/50 pt-6 sm:flex-row sm:items-center sm:justify-between">
          <p className="text-xs text-muted-foreground">
            {SITE.name} v{SITE.version} is released under the{" "}
            <a
              href={`${SITE.github}/blob/main/LICENSE`}
              target="_blank"
              rel="noopener noreferrer"
              className="text-foreground underline decoration-border underline-offset-2 transition-colors duration-200 hover:decoration-accent"
            >
              {SITE.license} license
            </a>
            . Made for {SITE.platforms}.
          </p>
          <BackToTop />
        </div>

        <div
          aria-hidden="true"
          className="pointer-events-none mt-14 select-none overflow-hidden"
        >
          <p className="footer-wordmark bg-gradient-to-b from-muted to-transparent bg-clip-text text-center text-[clamp(4rem,15vw,11rem)] font-bold leading-[0.82] tracking-tight text-transparent">
            {SITE.name}
          </p>
        </div>
      </div>
    </footer>
  );
}
