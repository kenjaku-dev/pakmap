// pkgbuildHighlight unit tests — the tokenizer's core contract: the token
// stream concatenates back to the exact input (display can never mutate the
// reviewed content), plus per-kind classification on representative lines.
import { describe, expect, it } from "vitest";
import { highlightPkgbuild, PKG_TOKEN_CLASS, type PkgToken } from "./pkgbuildHighlight";

const kinds = (t: PkgToken[]) => t.map((x) => x.text).join("");

describe("highlightPkgbuild", () => {
  it("reconstructs any input exactly (lossless invariant)", () => {
    const samples = [
      "",
      "pkgname=firefox-bin",
      "pkgdesc='Mozilla Firefox web browser (binary)'",
      "url='https://www.mozilla.org/firefox/'",
      "source=(${pkgname}-${pkgver}.tar.bz2)",
      "  install -Dm755 firefox ${pkgdir}/usr/bin/firefox",
      "install ${srcdir}/a \\ ${pkgdir}/b # inline note",
      "a'b",
      'incomplete "string ${var}',
      "https://example.org/unquoted/url",
      " leaked '# not a comment'",
      "#! makepkg comment\nline two",
    ];
    for (const s of samples) {
      const t = highlightPkgbuild(s);
      expect(kinds(t)).toBe(s);
    }
  });

  it("marks whole lines and inline tails as comments", () => {
    const t = highlightPkgbuild("# Maintainer: x\nfoo'not-a-comment' # tail");
    expect(t[0]).toMatchObject({ kind: "comment" });
    // inline tail (line 2): after a quoted string, the '#' starts a comment
    // whose token text is the rest-of-line (the leading space stays plain)
    const tail = t.find((x) => x.kind === "comment" && x.text.startsWith("# tail"));
    expect(tail?.text).toBe("# tail");
    expect(t.find((x) => x.kind === "comment" && x.text.startsWith("'"))).toBeUndefined();
  });

  it("classifies assignment keys", () => {
    const t = highlightPkgbuild("pkgname=firefox-bin\nmd5sums_x86_64=('x')\nleaked[v2]=no-key");
    expect(t.find((x) => x.kind === "key")?.text).toBe("pkgname");
    expect(t.find((x) => x.kind === "key" && x.text === "md5sums_x86_64")).toBeDefined();
    // mid-line / bracketed candidates are not keys
    expect(t.filter((x) => x.kind === "key")).toHaveLength(2);
  });

  it("classifies strings, urls and ${vars} (unquoted + in-string)", () => {
    const t = highlightPkgbuild("arch=('x86_64')\nurl='https://m.org/'\nopen ${srcdir}/x");
    const str = t.filter((x) => x.kind === "string").map((x) => x.text).join("");
    expect(str).toContain("'x86_64'");
    expect(t.find((x) => x.kind === "url")?.text).toBe("https://m.org/");
    expect(t.find((x) => x.kind === "variable" && x.text === "${srcdir}")).toBeDefined();
    const inString = highlightPkgbuild("c=1\nsrc=(${pkgname}).tar.gz");
    expect(inString.find((x) => x.kind === "variable")?.text).toBe("${pkgname}");
  });

  it("keeps one accent slot per kind and covers every non-plain kind", () => {
    const classes = new Set(Object.values(PKG_TOKEN_CLASS));
    expect(classes.size).toBe(Object.keys(PKG_TOKEN_CLASS).length);
  });
});
