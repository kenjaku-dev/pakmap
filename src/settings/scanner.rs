//! Lexical key scanner (plan §1.5).
//!
//! Mirrors `Config::strip_behavior_section`'s line-based discipline
//! (`src/config.rs:451`): TOML is line-delimited for structure, so a line
//! filter is exact for section/gating needs. The scanner REPORTS keys, it
//! does not parse values — values always come from serde's parse of the same
//! file, so a scanner gap can never corrupt a value; a miss on an unknown key
//! only yields one warning fewer (fail-open, plan D6: never blocks load).

/// One explicit `key =` line. `key` is the canonical dotted path
/// (`"search.limit"`); `line_no` is 1-based.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub key: String,
    pub line_no: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scan {
    /// Explicit keys, in file order. Duplicate keys appear once per line.
    pub keys: Vec<Found>,
}

/// Scan `source` for `[section]` headers and bare/quoted/dotted `key =`
/// lines within a table. Handles `[x]`/`[x.y]`/`[[x]]`/`["x.y"]` headers,
/// bare keys with dashes, inline arrays and multi-line bracket/quote
/// continuation (a value's continuation lines are never read as keys).
/// Garbage lines yield nothing — the scanner fails open, never fatal.
pub fn scan(source: &str) -> Scan {
    let mut out = Scan::default();
    let mut section: Vec<String> = Vec::new();
    // Continuation state: n = open brackets; quote = open quote char on the
    // key-value line. While active, lines never introduce keys.
    let mut open_brackets = 0usize;
    let mut in_multiline_string: Option<String> = None;
    let mut key_line_continues = false;

    for (idx, line) in source.lines().enumerate() {
        let line_no = idx + 1;
        if key_line_continues {
            // Close tracking: derive balance from this line's raw bytes.
            let (b, quote) = balance(line, open_brackets, &mut in_multiline_string);
            open_brackets = b;
            in_multiline_string = quote;
            key_line_continues = b > 0 || in_multiline_string.is_some();
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            open_brackets = 0;
            continue;
        }
        if starts_header(trimmed) {
            open_brackets = 0;
            if let Some(path) = parse_header(trimmed) {
                section = path;
            }
            continue;
        }
        if let Some((key, rest, _eq)) = split_key_value(trimmed) {
            let full = dotted(&section, &key);
            out.keys.push(Found { key: full, line_no });
            let (b, quote) = balance(rest, 0usize, &mut in_multiline_string);
            open_brackets = b;
            // A key line cannot be inside a string, so the quote state this
            // line produced (an opened `"""` delimiter) is authoritative.
            in_multiline_string = quote;
            key_line_continues = b > 0 || in_multiline_string.is_some();
        } else {
            // Not a key, not a header, not a comment: garbage → skip.
        }
    }
    out
}

fn starts_header(line: &str) -> bool {
    line == "[[" || (line.starts_with('[') && !line.starts_with("[["))
}

/// Header → dotted section path. `[[x]]` (array of tables) and quoted
/// components resolve the same as plan §1.5 requires. Garbage → None.
/// `pub(super)`: the writer reuses the read path's exact header resolution.
pub(super) fn parse_header(line: &str) -> Option<Vec<String>> {
    let t = line.trim();
    let body = if let Some(rest) = t.strip_prefix("[[") {
        rest.strip_suffix("]]")?
    } else {
        t.strip_prefix('[')?.strip_suffix(']')?
    };
    Some(split_dotted(body))
}

/// Split a dotted path on unquoted `.`; each component loses its quotes.
fn split_dotted(body: &str) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for ch in body.chars() {
        match (quote, ch) {
            (None, '"') | (None, '\'') => quote = Some(ch),
            (Some(q), ch) if ch == q => quote = None,
            (None, '.') => parts.push(std::mem::take(&mut cur)),
            (_, ch) if quote.is_some() || ch == '.' => cur.push(ch),
            (None, ch) => cur.push(ch),
            (Some(_), _) => {}
        }
    }
    parts.push(cur);
    parts.into_iter().filter(|p| !p.trim().is_empty()).collect()
}

/// `key = value…` → (key-part, rest-of-line, byte index of the `=`) where
/// neither a leading `#` comment nor quotes hide the `=`. Comments with `=`
/// inside (e.g. `# limit = 5`) never split — comment lines are skipped
/// before this runs, and the FIRST unquoted `=` on a real line ends the key
/// part. `pub(super)`: the writer (`write.rs`) reuses the same lexical
/// discipline plus the `=` offset for byte-surgical value replacement.
pub(super) fn split_key_value(line: &str) -> Option<(String, &str, usize)> {
    let bytes = line.as_bytes();
    let mut quote: Option<char> = None;
    let mut eq = None;
    for (i, ch) in line.char_indices() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => {}
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if ch == '=' => {
                eq = Some(i);
                break;
            }
            None => {}
        }
    }
    let eq = eq?;
    // Sanity: bytes[..eq] must contain no `#` (that would be a comment
    // fragment mid-line, e.g. `x # = 5`) — TOML forbids it; fail open.
    if bytes[..eq].contains(&b'#') {
        return None;
    }
    let key = line[..eq].trim();
    if key.is_empty() || !is_toml_key(key) {
        return None;
    }
    Some((key.to_string(), line[eq + 1..].trim(), eq))
}

/// A key part is legal TOML: dotted components are bare keys
/// (`[A-Za-z0-9_-]+`) or quoted strings, split on unquoted dots. Anything
/// else on a line before `=` is prose, not a key — the scanner reports
/// nothing for it (§1.5 fail-open).
fn is_toml_key(key: &str) -> bool {
    fn comp_ok(comp: &str) -> bool {
        let b = comp.as_bytes();
        match b.first() {
            None => false,
            Some(b'"') | Some(b'\'') => true, // quoted comp (checked during walk)
            Some(_) => b
                .iter()
                .all(|&c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'),
        }
    }
    let chs: Vec<char> = key.chars().collect();
    let mut comps = 0usize;
    let mut comp_start = 0usize;
    let mut quote: Option<char> = None;
    // Sentinel dot closes the final component.
    for (i, ch) in chs
        .iter()
        .chain(std::iter::repeat(&'.'))
        .take(chs.len() + 1)
        .enumerate()
    {
        match quote {
            Some(q) if *ch == q => quote = None,
            Some(_) => {}
            None if *ch == '.' => {
                let comp: String = chs[comp_start..i].iter().collect();
                if !comp_ok(&comp) {
                    return false;
                }
                comps += 1;
                comp_start = i + 1;
            }
            None if *ch == '"' || *ch == '\'' => quote = Some(*ch),
            None => {}
        }
    }
    comps > 0
}

/// Count remaining open brackets/after this chunk and detect an unterminated
/// string. Body covers the `value` part of the line (already past `=`) or a
/// continuation line. Tracks `"`/`'` strings (with `"` escapes) and
/// `"""`/`'''` multi-line strings; `#` comments out of a string end scanning.
/// `pub(super)`: the writer reuses the identical continuation tracking so a
/// statement's line extent is decided one way in both modules.
pub(super) fn balance(
    chunk: &str,
    open: usize,
    multiline: &mut Option<String>,
) -> (usize, Option<String>) {
    // Multi-line string continuation: hunt the closing delimiter.
    if let Some(delim) = multiline.clone() {
        if let Some(pos) = chunk.find(&delim) {
            let after = &chunk[pos + delim.len()..];
            // Content after a closing delimiter belongs to the same line's
            // remainder (inline comment) — treat as closed.
            *multiline = None;
            return balance_rest(after, open);
        }
        return (open, multiline.clone());
    }
    balance_rest(chunk, open)
}

fn balance_rest(chunk: &str, open: usize) -> (usize, Option<String>) {
    let mut open = open;
    let mut i = 0;
    let bytes = chunk.as_bytes();
    let n = bytes.len();
    while i < n {
        // Comments outside a string and outside brackets end the scan of
        // this logical line.
        if bytes[i] == b'#' && open == 0 {
            return (open, None);
        }
        if bytes[i] == b'[' || bytes[i] == b'{' {
            open += 1;
            i += 1;
            continue;
        }
        if bytes[i] == b']' {
            open = open.saturating_sub(1);
            i += 1;
            continue;
        }
        if bytes[i] == b'}' {
            open = open.saturating_sub(1);
            i += 1;
            continue;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let q = bytes[i] as char;
            // Triple-quote multi-line strings ("""/''')
            if i + 2 < n && bytes[i + 1] == bytes[i + 2] && bytes[i + 1] == bytes[i] {
                let delim = q.to_string().repeat(3);
                match chunk[i + 3..].find(&delim) {
                    Some(rel) => {
                        i = i + 3 + rel + 3;
                        continue;
                    }
                    None => {
                        // Unterminated multi-line string: track its delimiter
                        // until a later line closes it.
                        return (open, Some(delim));
                    }
                }
            }
            let mut j = i + 1;
            while j < n {
                if bytes[j] == b'\\' && q == '"' {
                    j += 2;
                    continue;
                }
                if bytes[j] as char == q {
                    break;
                }
                if bytes[j] == b'#' {
                    break;
                }
                j += 1;
            }
            i = j + 1;
            continue;
        }
        i += 1;
    }
    (open, None)
}

/// Join current-table path with the key line's own dotted path.
/// `pub(super)`: the writer matches keys by the same dotted form.
pub(super) fn dotted(section: &[String], key: &str) -> String {
    let key_parts = split_dotted(key);
    let mut all: Vec<String> = section.to_vec();
    all.extend(key_parts);
    all.join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(src: &str) -> Vec<String> {
        scan(src).keys.iter().map(|f| f.key.clone()).collect()
    }

    #[test]
    fn inline_arrays_are_one_key_not_keys() {
        let src = "[tui]\npopup_info = [70, 60]\npopup = { a = 1, b = 2 }\n";
        assert_eq!(keys(src), ["tui.popup_info", "tui.popup"]);
    }

    #[test]
    fn hash_comments_with_equals_inside_are_skipped() {
        let src = "[search]\n# limit = 5 (comment)\nlimit = 7 # also = comment\n";
        assert_eq!(keys(src), ["search.limit"]);
    }

    #[test]
    fn quoted_keys_and_quoted_headers() {
        // Legal quoted forms: quoted key spellings and quoted header parts
        // all resolve to their dotted canonical paths.
        let src = "[\"weird section\"]\n\"k\" = 1\n\"a.b\" = 3\n[dotted.\"hdr\"]\nk3 = 3\n";
        let ks = keys(src);
        assert!(ks.contains(&"weird section.k".to_string()));
        assert!(ks.contains(&"weird section.a.b".to_string()));
        assert!(ks.contains(&"dotted.hdr.k3".to_string()));
    }

    #[test]
    fn bracketed_prose_is_not_a_key() {
        // `['a.b'] = 2` is not legal TOML syntax for a key line; the scanner
        // fails open and reports nothing rather than crashing or inventing
        // keys (plan §1.5).
        let src = "['ok'.'inner'] = 2\n";
        assert!(keys(src).is_empty());
    }

    #[test]
    fn dotted_keys_carry_their_path() {
        // Top-level dotted key: section = search.
        let src = "search.limit = 4\n[search]\nlimit = 5\n";
        assert_eq!(keys(src), ["search.limit", "search.limit"]);
    }

    #[test]
    fn subtable_headers_land_in_the_family() {
        let src = "[search.deeper]\nkey = 1\n[theme]\ntext = \"white\"\n";
        assert_eq!(keys(src), ["search.deeper.key", "theme.text"]);
    }

    #[test]
    fn duplicate_keys_are_reported_per_line() {
        let src = "[search]\nlimit = 1\nlimit = 2\n";
        // serde rejects these fully; the scanner still reports every line and
        // resolve turns the parse failure into a warn+skip (fail-open).
        assert_eq!(keys(src), ["search.limit", "search.limit"]);
        assert_eq!(scan(src).keys.first().unwrap().line_no, 2);
        assert_eq!(scan(src).keys.get(1).unwrap().line_no, 3);
    }

    #[test]
    fn multiline_array_and_strings_never_become_keys() {
        // Fully valid TOML: multi-line arrays and `"""` strings keep their
        // continuation lines from being read as keys.
        let src = concat!(
            "[tui]\n",
            "popup_info = [\n",
            "  70,\n",
            "  60,\n",
            "]\n",
            "tick_chars = \"\"\"\n",
            "multi\"\"\"\n",
            "debounce_ms = 400\n",
            "[x]\nempty_string = \"\"\n",
            "[x.close]\ny = 3\n",
        );
        let ks = keys(src);
        assert!(ks.contains(&"tui.popup_info".to_string()));
        assert!(ks.contains(&"tui.tick_chars".to_string()));
        assert!(ks.contains(&"tui.debounce_ms".to_string()));
        assert!(ks.contains(&"x.empty_string".to_string()));
        // No continuation line surfaced as a key ("70,"/"60,"/"multi").
        assert!(!ks.iter().any(|k| k.contains("multi")));
        assert_eq!(ks.len(), 5);
        assert_eq!(scan(src).keys.last().unwrap().key, "x.close.y");
    }

    #[test]
    /// Unterminated quotes and stray brackets (invalid TOML) never panic and
    /// keep following well-formed sections readable after the file is
    /// rejected wholesale by serde (the scanner only feeds warnings).
    fn invalid_toml_fails_open_without_cascades() {
        let src = "[x]\nnot = \"partial [\nstill = 1\n";
        let _ = keys(src); // must not panic; exact content is unspecified
        assert!(
            keys("[x]\nvery = \"bad\n[y]\nok = 1\n")
                .iter()
                .any(|k| k.contains("y.ok"))
        );
    }

    #[test]
    /// §1.5 fail-open contract: a garbage file yields nothing, never an error.
    fn garbage_fails_open() {
        assert_eq!(keys("this is not toml ((( =="), Vec::<String>::new());
        assert_eq!(keys(""), Vec::<String>::new());
    }

    #[test]
    fn keys_with_dashes_and_inline_comments() {
        let src = "[behavior]\nnews_cache_secs = 86400  # 1 day\nremove_flags = \"Rs\"\n";
        assert_eq!(
            keys(src),
            ["behavior.news_cache_secs", "behavior.remove_flags"]
        );
    }
}
