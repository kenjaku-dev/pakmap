import { CopyButton } from "./ui/CopyButton";
import { Section } from "./ui/Section";
import { SpotlightCard } from "./ui/SpotlightCard";

const BLOCKS = [
  {
    label: "From source (recommended)",
    command: `git clone https://github.com/kenjaku-dev/pakmap
cd pakmap
cargo install --path . --force
# no clone: cargo install --git https://github.com/kenjaku-dev/pakmap --locked pakmap
# (bare 'cargo install --git <repo>' fails: workspace ships two binaries)`,
  },
  {
    label: "Desktop GUI",
    command: `git clone https://aur.archlinux.org/pakmap-gui.git
cd pakmap-gui && makepkg -si
# or: pakmap-gui tarball from GitHub releases`,
  },
  {
    label: "Everyday use",
    command: `pakmap                 # TUI (TTY)
pakmap-gui             # desktop app
pakmap firefox --no-tui  # CLI / scripts
pakmap --init-config     # write config.toml`,
  },
] as const;

const DEPS =
  "Requires pacman 5.1+ (libalpm), pacman-contrib, git, base-devel for AUR builds, Rust 1.90+. Flatpak (optional) enables --source flatpak. The desktop GUI additionally needs webkit2gtk-4.1 and polkit.";

function CommandLine({ line }: { line: string }) {
  const hashIndex = line.indexOf("#");
  const command = hashIndex === -1 ? line : line.slice(0, hashIndex);
  const comment = hashIndex === -1 ? "" : line.slice(hashIndex);

  return (
    <span className="block">
      <span aria-hidden="true" className="select-none text-accent">
        ${" "}
      </span>
      <span className="text-foreground">{command}</span>
      {comment && (
        <span className="text-muted-foreground">{comment}</span>
      )}
    </span>
  );
}

export function Install() {
  return (
    <Section
      id="install"
      labelledBy="install-heading"
      className="mx-auto w-full max-w-5xl px-5 py-16 sm:px-8 sm:py-24"
    >
      <div className="mb-10 max-w-2xl">
        <p className="mb-3 text-[11px] font-medium uppercase tracking-[0.25em] text-accent">
          Install
        </p>
        <h2
          id="install-heading"
          className="text-balance text-2xl font-semibold tracking-tight text-foreground sm:text-3xl"
        >
          Install in one go
        </h2>
        <p className="mt-3 text-sm leading-relaxed text-muted-foreground sm:text-base">
          Stripped release build is a few megabytes. Refresh sync DB once before
          the first repo search:{" "}
          <code className="text-accent-secondary">sudo pacman -Sy</code>
        </p>
      </div>

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        {BLOCKS.map((block) => (
          <SpotlightCard
            key={block.label}
            className="flex flex-col overflow-hidden rounded-xl border border-border/60 bg-card/80 backdrop-blur-sm transition-colors duration-300 hover:border-accent/40"
          >
            <div className="flex items-center gap-2 border-b border-border/60 bg-muted/40 px-3 py-2">
              <span
                aria-hidden="true"
                className="h-2 w-2 rounded-full bg-destructive/70"
              />
              <span
                aria-hidden="true"
                className="h-2 w-2 rounded-full bg-yellow-500/60"
              />
              <span
                aria-hidden="true"
                className="h-2 w-2 rounded-full bg-accent/60"
              />
              <h3 className="ml-1 flex-1 truncate text-[11px] font-medium text-muted-foreground">
                {block.label}
              </h3>
              <CopyButton text={block.command} label={`Copy ${block.label}`} />
            </div>
            <pre className="flex-1 overflow-x-auto p-4 text-[11px] leading-loose sm:text-xs">
              <code>
                {block.command.split("\n").map((line) => (
                  <CommandLine key={line} line={line} />
                ))}
              </code>
            </pre>
          </SpotlightCard>
        ))}
      </div>

      <p className="mt-5 text-xs leading-relaxed text-muted-foreground">
        {DEPS}
      </p>
    </Section>
  );
}
