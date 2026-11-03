//! Per-key validation + nearest-key suggestion (plan §1, §2).
//!
//! Validation is strict at the WRITING door (`config set`) so bad values
//! never reach disk; the READ path stays fail-open everywhere (D6): parsing
//! never fails on an invalid value in a file, it only warns. Runtime clamps
//! (poll/debounce/timeout, clean_keep ≥ 1) are mirrored so a written value
//! matches what the op layer would actually apply.

use super::catalog::{EnumVal, Kind, SettingMeta};

/// Allowlist shared with `Config::remove_flag_arg` semantics (`src/config.rs`
/// allowlist: R|Rs|Rns|Ru|Rsu|Rn|Rc|Rdd).
const REMOVE_FLAG_ALLOW: &[&str] = &["R", "Rs", "Rns", "Ru", "Rsu", "Rn", "Rc", "Rdd"];
const DIFF_PAGER_VALUES: &[EnumVal] = &[
    EnumVal {
        name: "auto",
        desc: "auto-detect",
    },
    EnumVal {
        name: "bat",
        desc: "bat",
    },
    EnumVal {
        name: "less",
        desc: "less",
    },
    EnumVal {
        name: "cat",
        desc: "cat",
    },
];

/// Validate `raw` for `meta`. `Ok(())` = acceptable.
pub fn validate(meta: &SettingMeta, raw: &str) -> Result<(), String> {
    validate_reason(meta, raw).map_or(Ok(()), Err)
}

/// Compose the failure message when `raw` is unacceptable.
fn validate_reason(meta: &SettingMeta, raw: &str) -> Option<String> {
    let t = raw.trim();
    let kind_reason = match meta.kind {
        Kind::Bool => match t {
            "true" | "false" => None,
            _ => Some("must be true or false".into()),
        },
        Kind::U16 => integer_check(t, u16::MAX.into()),
        Kind::U32 => match t.parse::<u32>() {
            Ok(v) if v >= 1 => None,
            Ok(_) => Some("expected at least 1".into()),
            Err(_) => Some("expected a non-negative number".into()),
        },
        Kind::U64 => integer_check(t, u64::MAX),
        Kind::Usize => integer_check(t, usize::MAX.try_into().unwrap_or(u64::MAX)),
        Kind::Enum(vals) => enum_reason(vals, t),
        Kind::Style => None, // fail-open like parse_style (unknown → no color)
        Kind::Text => None,  // free-form text; key-specific narrows below
        Kind::OptText => None,
        Kind::PairU16 => pair_check(t),
    };
    match kind_reason {
        Some(msg) => Some(msg),
        None => key_specific_check(meta, t),
    }
}

fn integer_check(t: &str, max: u64) -> Option<String> {
    match t.parse::<u64>() {
        Ok(v) if v <= max => None,
        Ok(_) => Some("value too large for this setting".into()),
        Err(_) => Some("expected a non-negative number".into()),
    }
}

fn pair_check(t: &str) -> Option<String> {
    let parts: Vec<&str> = t.split(',').map(str::trim).collect();
    if parts.len() != 2 {
        return Some("expected two comma-separated numbers (cols,rows)".into());
    }
    for p in parts {
        if p.parse::<u16>().is_err() {
            return Some("expected two comma-separated u16 values".into());
        }
    }
    None
}

fn enum_reason(vals: &[EnumVal], t: &str) -> Option<String> {
    if vals.iter().any(|v| v.name == t) {
        None
    } else {
        Some(format!(
            "expected one of [{}]",
            vals.iter().map(|v| v.name).collect::<Vec<_>>().join("|")
        ))
    }
}

/// Numeric clamps mirrored from the op layer (`tui/app.rs`: poll 50..=1000,
/// debounce 50..=5000; `news.rs`: timeout 3..=30; keep ≥ 1).
///
/// `numeric_clamp` is the SINGLE SOURCE for those limits: `key_specific_check`
/// enforces them and the GUI's backend wires them for the number-input badges
/// (plan §3) — adding a clamp without updating this match fails the range test.
fn uint_range_check(t: &str, min: u64, max: u64) -> Option<String> {
    match t.parse::<u64>() {
        Ok(v) if (min..=max).contains(&v) => None,
        Ok(_) => Some(format!("expected {min}..={max}")),
        Err(_) => Some("expected a non-negative number".into()),
    }
}

/// The documented numeric clamps per key, as an inclusive `(min, max)` tuple.
pub fn numeric_clamp(key: &str) -> Option<(u64, u64)> {
    match key {
        "tui.debounce_ms" => Some((50, 5000)),
        "tui.poll_ms" => Some((50, 1000)),
        "search.timeout_secs" => Some((3, 30)),
        "behavior.verbose" => Some((0, u8::MAX.into())),
        _ => None,
    }
}

/// Key-specific checks needing per-key context. Enum-flavored keys and
/// allowlist/shape checks live here; everything else passes through.
fn key_specific_check(meta: &SettingMeta, t: &str) -> Option<String> {
    // numeric clamps: single source (`numeric_clamp`) — engine tests pin this
    // against the op-layer semantics recorded in the function's doc.
    if let Some((min, max)) = numeric_clamp(meta.key) {
        return uint_range_check(t, min, max);
    }
    match meta.key {
        "behavior.diff_pager" => {
            if t.is_empty() {
                None
            } else {
                enum_reason(DIFF_PAGER_VALUES, t)
            }
        }
        "behavior.remove_flags" => {
            let core = t.trim().trim_start_matches('-');
            if REMOVE_FLAG_ALLOW.contains(&core) {
                None
            } else {
                Some(format!("allowed values: {}", REMOVE_FLAG_ALLOW.join("|")))
            }
        }
        "behavior.aur_rpc" => {
            if t.is_empty() || t.starts_with("http://") || t.starts_with("https://") {
                None
            } else {
                Some("must start with http:// or https:// (blank = default)".into())
            }
        }
        _ => None,
    }
}

/// Nearest catalog key within plain Levenshtein distance ≤ 2 (plan §2), if
/// any. Ties keep catalog order.
pub fn nearest_key(typo: &str) -> Option<&'static str> {
    let mut best: Option<(&'static str, usize)> = None;
    for meta in super::catalog::CATALOG {
        if let Some(d) = levenshtein_2(typo, meta.key)
            && d <= 2
            && best.filter(|(_, dd)| *dd <= d).is_none()
        {
            best = Some((meta.key, d));
        }
    }
    best.map(|(k, _)| k)
}

/// Levenshtein distance with early cutoff at 2.
fn levenshtein_2(a: &str, b: &str) -> Option<usize> {
    let av: Vec<char> = a.chars().collect();
    let bv: Vec<char> = b.chars().collect();
    if av.len().abs_diff(bv.len()) > 2 {
        return None; // cannot be within cutoff
    }
    let mut prev = Vec::<usize>::with_capacity(bv.len() + 1);
    (0..=bv.len()).for_each(|i| prev.push(i));
    for (i, ca) in av.iter().enumerate() {
        let mut row = Vec::<usize>::with_capacity(bv.len() + 1);
        row.push(i + 1);
        for (j, cb) in bv.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != cb);
            row.push(sub.min(row[j] + 1).min(prev[j + 1] + 1));
        }
        if row.iter().min().unwrap_or(&usize::MAX) > &2 {
            return None;
        }
        prev = row;
    }
    Some(prev[bv.len()])
}

#[cfg(test)]
mod tests {
    use super::super::catalog::{CATALOG, SettingMeta};
    use super::*;

    fn v(key: &str, val: &str) -> Result<(), String> {
        let m = SettingMeta::lookup(key).ok_or("no key")?;
        validate(m, val)
    }

    /// `numeric_clamp` is the single source (plan §3 badges): pin the tuples
    /// and make sure exactly the four clamped keys are listed — a new clamp
    /// in `key_specific_check` without this list breaks the parity.
    #[test]
    fn numeric_clamp_covers_every_clamped_key_exactly() {
        assert_eq!(numeric_clamp("tui.debounce_ms"), Some((50, 5000)));
        assert_eq!(numeric_clamp("tui.poll_ms"), Some((50, 1000)));
        assert_eq!(numeric_clamp("search.timeout_secs"), Some((3, 30)));
        assert_eq!(numeric_clamp("behavior.verbose"), Some((0, 255)));
        for meta in CATALOG {
            if meta.key != "tui.debounce_ms"
                && meta.key != "tui.poll_ms"
                && meta.key != "search.timeout_secs"
                && meta.key != "behavior.verbose"
            {
                assert_eq!(
                    numeric_clamp(meta.key),
                    None,
                    "unexpected clamp entry for {}",
                    meta.key
                );
            }
        }
    }

    #[test]
    fn clamped_numbers_reject_out_of_range() {
        assert!(v("tui.debounce_ms", "0").is_err());
        assert!(v("tui.debounce_ms", "5000").is_ok());
        assert!(v("tui.debounce_ms", "5001").is_err());
        assert!(v("tui.poll_ms", "1000").is_ok());
        assert!(v("tui.poll_ms", "49").is_err_and(|e| e.contains("50..=1000")));
        assert!(v("search.timeout_secs", "3").is_ok());
        assert!(v("search.timeout_secs", "31").is_err_and(|e| e.contains("3..=30")));
        assert!(v("behavior.clean_keep", "1").is_ok());
        assert!(v("behavior.clean_keep", "0").is_err());
    }

    #[test]
    fn enums_strict() {
        assert!(v("search.source", "aur").is_ok());
        assert!(v("search.source", "repo2").is_err_and(|e| e.contains("all")));
        assert!(v("gui.theme", "dark").is_ok());
        assert!(v("gui.theme", "Dark").is_err());
        assert!(v("tui.border", "double").is_ok());
        assert!(v("tui.border", "dashed").is_err());
    }

    #[test]
    fn remove_flags_allowlist() {
        assert!(v("behavior.remove_flags", "Rs").is_ok());
        assert!(v("behavior.remove_flags", "-Rns").is_ok());
        assert!(v("behavior.remove_flags", "--noconfirm; rm -rf").is_err());
    }

    #[test]
    fn aur_rpc_shape() {
        assert!(v("behavior.aur_rpc", "").is_ok());
        assert!(v("behavior.aur_rpc", "https://aur.example/v5").is_ok());
        assert!(v("behavior.aur_rpc", "http://x").is_ok());
        assert!(v("behavior.aur_rpc", "ftp://x").is_err());
    }

    #[test]
    fn opt_text_empty_is_default() {
        assert!(v("tui.highlight_symbol", "").is_ok());
        assert!(v("tui.highlight_symbol", "▸ ").is_ok());
        assert!(v("behavior.diff_pager", "auto").is_ok());
        assert!(v("behavior.diff_pager", "wanted").is_err());
    }

    #[test]
    fn bools_and_pairs() {
        assert!(v("search.regex", "true").is_ok());
        assert!(v("search.regex", "True").is_err());
        assert!(v("tui.popup_info", "70,60").is_ok());
        assert!(v("tui.popup_info", "70").is_err());
    }

    #[test]
    fn nearest_suggestion_found() {
        assert_eq!(nearest_key("search.limt"), Some("search.limit"));
        // "behaviour" → "behavior" is distance 1 over the prefix.
        assert_eq!(nearest_key("behaviour.readonly"), Some("behavior.readonly"));
        assert_eq!(nearest_key("writeonly"), None, "distance too far");
    }
}
