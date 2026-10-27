// B-phase (v0.9.1) PKGBUILD syntax highlighting — a dependency-free,
// single-pass tokenizer (plan §3: ~2KB budget, no prism/shiki). Pure module
// (vitest-tested): tokens always concatenate back to the exact input, so the
// rendering path can never mutate the reviewed content. PkgbuildSection maps
// kinds to semantic Tailwind tokens; nothing here touches the diff panel.
export type PkgTokenKind = "plain" | "comment" | "key" | "string" | "url" | "variable";

export interface PkgToken {
  text: string;
  kind: PkgTokenKind;
}

const URL_RE = /^https?:\/\/[^\s"'\\)]+/;
const VAR_RE = /^\$\{?[A-Za-z_][A-Za-z0-9_]*\}?/;
const KEY_RE = /^[A-Za-z_][A-Za-z0-9_]*@?=/;

function classifyInString(rest: string): PkgToken {
  const url = URL_RE.exec(rest);
  if (url) return { text: url[0], kind: "url" };
  const vari = VAR_RE.exec(rest);
  if (vari) return { text: vari[0], kind: "variable" };
  return { text: rest[0], kind: "string" };
}

function pushString(line: string, start: number, quote: string, out: PkgToken[]): number {
  out.push({ text: quote, kind: "string" });
  let i = start + quote.length;
  let buf = "";
  while (i < line.length) {
    const ch = line[i];
    // backslash escapes only live inside double quotes in bash — in either
    // case an escaped close-quote must not end the string
    if (ch === "\\" && i + 1 < line.length && line[i + 1] === quote) {
      buf += ch + line[i + 1];
      i += 2;
      continue;
    }
    if (ch === quote) break;
    buf += ch;
    i += 1;
  }
  if (buf !== "") {
    // chunk the gap on in-string URLs / ${var} references
    let j = 0;
    while (j < buf.length) {
      const tok = classifyInString(buf.slice(j));
      if (j + tok.text.length > buf.length) {
        // URL_RE matched past the end of the buffer (impossible by
        // construction) — treat single-char to stay in-bounds
        out.push({ text: buf[j], kind: "string" });
        j += 1;
      } else {
        out.push(tok);
        j += tok.text.length;
      }
    }
  }
  if (i < line.length) out.push({ text: quote, kind: "string" });
  return Math.min(i + 1, line.length);
}

function pushLine(line: string, out: PkgToken[]): void {
  // whole-line comment (leading whitespace tolerated)
  if (/^\s*#/.test(line)) {
    out.push({ text: line, kind: "comment" });
    return;
  }
  let i = 0;
  let plain = "";
  const flush = () => {
    if (plain !== "") {
      out.push({ text: plain, kind: "plain" });
      plain = "";
    }
  };
  while (i < line.length) {
    const rest = line.slice(i);
    // key: identifier= at line start (before any non-space content)
    if (plain.trim() === "" && KEY_RE.test(rest)) {
      const eq = rest.indexOf("=");
      flush();
      out.push({ text: rest.slice(0, eq), kind: "key" });
      plain += "=";
      i += eq + 1;
      continue;
    }
    if (rest[0] === "'" || rest[0] === '"') {
      flush();
      i = pushString(line, i, rest[0], out);
      continue;
    }
    if (rest[0] === "#") {
      flush();
      out.push({ text: rest, kind: "comment" });
      return;
    }
    const url = plain.trim() !== "" ? null : URL_RE.exec(rest);
    if (url) {
      flush();
      out.push({ text: url[0], kind: "url" });
      i += url[0].length;
      continue;
    }
    const vari = VAR_RE.exec(rest);
    if (vari) {
      flush();
      out.push({ text: vari[0], kind: "variable" });
      i += vari[0].length;
      continue;
    }
    plain += line[i];
    i += 1;
  }
  flush();
}

/** Tokenize a PKGBUILD for display. Contract: tokens concat back to `content`
 * exactly (whitespace/newlines included) — verified by tests. */
export function highlightPkgbuild(content: string): PkgToken[] {
  const out: PkgToken[] = [];
  const lines = content.split("\n");
  lines.forEach((line, idx) => {
    if (idx > 0) out.push({ text: "\n", kind: "plain" });
    pushLine(line, out);
  });
  return out;
}

/** Semantic-token → Tailwind class mapping (two-tier token palette; ≤3 accents). */
export const PKG_TOKEN_CLASS: Record<Exclude<PkgTokenKind, "plain">, string> = {
  comment: "text-muted-foreground",
  key: "text-accent",
  string: "text-foreground",
  url: "text-accent-secondary underline decoration-border/60",
  variable: "text-accent-tertiary",
};
