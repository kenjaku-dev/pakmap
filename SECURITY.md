# Security Policy

## Supported versions

Only tagged releases get security fixes. Older versions are best-effort at
most; if you report against one, please re-verify against the latest tag
first.

| Version | Supported |
| ------- | --------- |
| latest tagged release | yes |
| older tags | no |

`install.sh` and the AUR packages are only ever advertised against the
pinned version(s) in the repo's version contract — the `--latest` flag in
`install.sh` keeps upgrades one command away for the same reason.

## Reporting a vulnerability

Please use **GitHub Security Advisories** (Repository → Security → Report a
vulnerability). That is the intended channel for pakmap, and it keeps
coordination private.

Please prefer private reporting over a public issue when what you found is
exploitable or could be attacker-controlled (a crafted package name, feed,
RPC endpoint, or file). If the finding is only a hardening deficiency with
no exploit path, a normal public issue is fine.

Include what you can of:

- the pakmap version (`pakmap --version`, or the GUI "about" version)
- which surface: CLI/TUI, GUI, `install.sh`
- steps or a PoC (a config file, a feed URL, a package name — whatever
  triggers it)
- impact (data read/written, persistence, what an attacker gains)

You'll get an acknowledgment within **7 days**, and a fix targeted for the
next patch release. If you would like credit, say how you want your name
written in the release notes; otherwise you stay anonymous.

Coordinated disclosure, by default: publicly disclose after the fix ships
in a tagged release. If the finding is actively exploited or high severity,
we can discuss a shorter timeline together.

## Scope

**In scope:**

- the `pakmap` CLI/TUI binary: alpm bindings usage, AUR RPC + news feed
  handling, shell-outs to external tools, config load path, pager/review
  flows, the `--json` boundary
- the `pakmap-gui` desktop app: Tauri IPC command surface, URL validation
  and opening, PKGBUILD display
- `install.sh`: download URL handling, checksum verification, install
  paths, privilege usage
- `packaging/aur/*` PKGBUILD templates (their packaged behavior)

**Out of scope:**

- vulnerabilities in dependencies — report them upstream, but a message
  with the CVE is still welcome if it impacts pakmap's use
- issues that require already-running arbitrary code as the user
- attacks on pacman/libalpm itself (Arch's truck, not pakmap's)
- missing hardening *ideas* with no plausible exploit path

## Built-in hardening

pakmap treats package names, descriptions, feed content, and remote
bodies as untrusted input, not data-for-a-developer's-eyes-only. A few
structural guarantees, in case you audit:

- `unwrap`/`expect` are **denied** by clippy in production code
  (`Cargo.toml [lints.clippy]`) — no panic expected on bad input
- shared gates in `src/safety/`: filesystem ownership checks, text
  sanitization, external-URL validation, HTTP body caps
- every background subprocess wait is bounded (`src/process.rs`) — no
  unbounded hangs; interactive installs intentionally inherit the terminal
- `--json` output is a stable byte-for-byte boundary: display-only
  sanitization never rewrites it

## Finding already-fixed issues

Security-relevant fixes land in patch releases with the `security:` commit
prefix — grep the changelog/log for `security:` to see what has been done.
