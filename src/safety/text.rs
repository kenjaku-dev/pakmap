//! UTF-8-safe truncation and terminal-neutral sanitization.
//!
//! - [`truncate_utf8`] / [`trim_partial_tail`] close the CWE-248 panic: byte
//!   caps used to cut straight through a multi-byte char (`[500]`) and crash
//!   the error-handling path itself.
//! - [`sanitize_terminal`] closes the CWE-150 class: raw AUR strings printed
//!   to a terminal can hide review lines with escapes/reorders. Display-only —
//!   JSON output and stored data are never sanitized.

/// Byte-cap cut landing on a UTF-8 char boundary. Returns the slice and
/// whether anything was cut. Pure, no panics: walks back at most a few bytes.
pub fn truncate_utf8(bytes: &[u8], max: usize) -> (&[u8], bool) {
    if bytes.len() <= max {
        return (bytes, false);
    }
    let mut cut = max;
    // A byte at `cut` with the continuation pattern (0b10xxxxxx) means the
    // cap would split a char — step back to the char start.
    while cut > 0 && bytes[cut] & 0xC0 == 0b1000_0000 {
        cut -= 1;
    }
    (&bytes[..cut], true)
}

/// Salvage the largest valid UTF-8 prefix of a byte-cap truncation: a cut can
/// end inside a 2–4 byte sequence and leave up to 3 orphan bytes at the tail.
/// Callers map any remaining failure (truly non-text content) to their
/// "not text" path.
#[must_use]
pub fn trim_partial_tail(bytes: &[u8]) -> &[u8] {
    if bytes.is_empty() || std::str::from_utf8(bytes).is_ok() {
        // Already a complete, valid sequence set — nothing to salvage.
        return bytes;
    }
    // Largest prefix first: skip=1 beats skip=2 beats skip=3.
    for skip in 1..=3usize {
        if let Ok(prefix) = std::str::from_utf8(&bytes[..bytes.len() - skip]) {
            return prefix.as_bytes();
        }
    }
    bytes
}

/// Error-context snippet for messages embedding untrusted data (AUR responses):
/// byte-cap cut on a char boundary, then terminal-neutralized, then lossy for
/// the (already corrupt) case where the tail is not valid UTF-8.
#[must_use]
pub fn error_snippet(input: &str, cap: usize) -> String {
    let (bytes, _) = truncate_utf8(input.as_bytes(), cap);
    let lossy = String::from_utf8_lossy(bytes);
    sanitize_terminal(&lossy)
}

/// Neutralize terminal-driving characters for display paths.
///
/// Survives: `'\n'`, `'\t'`, and every ordinary Unicode char (Arabic, CJK,
/// emoji… stay byte-identical). Removed: C0/C1 controls (ESC/CSI sequences,
/// OSC52 clipboard hijacks, cursor moves), DEL, and the Unicode format/bidi
/// set (RLM, LRM, embeddings, isolates) that can visually reorder or hide
/// rendered text. Display-only by design: JSON output passes data untouched.
#[must_use]
pub fn sanitize_terminal(input: &str) -> String {
    input.chars().filter(|c| is_display_safe(*c)).collect()
}

/// Keep-list decision for one char.
pub fn is_display_safe(c: char) -> bool {
    match c {
        '\n' | '\t' => true,
        _ => !(c.is_control() || is_hidden_bidi_format(c)),
    }
}

/// Unicode bidi/format characters with zero width whose only display effect
/// is reordering or hiding rendered text (concealing review lines).
fn is_hidden_bidi_format(c: char) -> bool {
    matches!(
        c,
        // ZWSP..RLM (incl. ZWNJ, ZWJ, LRM, RLM)
        '\u{200B}'..='\u{200F}'
        // LRE..RLO (bidi embeddings + explicit overrides)
        | '\u{202A}'..='\u{202E}'
        // LRI/RLI/FSI/PDI (bidi isolates)
        | '\u{2066}'..='\u{2069}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_ascii_and_flag() {
        assert_eq!(truncate_utf8(b"hello", 10), (&b"hello"[..], false));
        assert_eq!(truncate_utf8(b"hello", 3), (&b"hel"[..], true));
        assert_eq!(truncate_utf8(b"hello", 0), (&b""[..], true));
        assert_eq!(truncate_utf8(b"", 5), (&b""[..], false));
    }

    #[test]
    fn truncate_never_splits_multibyte_chars() {
        // Arabic + CJK + an emoji cluster: every cap cut must stay valid UTF-8.
        let text = "حزمة إلغاء delights 示例3️⃣".as_bytes();
        for cap in 0..=text.len() {
            let (cut, was_cut) = truncate_utf8(text, cap);
            assert!(
                std::str::from_utf8(cut).is_ok(),
                "cap {cap} produced invalid UTF-8"
            );
            assert_eq!(was_cut, cap < text.len());
            assert!(cut.len() <= cap);
        }
        let (cut, was_cut) = truncate_utf8(text, text.len());
        assert_eq!(cut, text, "cap == len keeps everything");
        assert!(!was_cut);
    }

    #[test]
    fn trim_partial_tail_salvages_shifted_sequences() {
        let text = "الجزء域".as_bytes();
        // Complete valid content passes through untouched (cap-aligned text
        // with no split char keeps every byte, including a trailing '\n').
        assert_eq!(trim_partial_tail(text), text);
        assert_eq!(trim_partial_tail(&b"pkgver=1\n"[..]), &b"pkgver=1\n"[..]);

        // 域 is 3 bytes (E5 9F 9F): cut inside it or drop it whole.
        assert_eq!(
            trim_partial_tail(&text[..text.len() - 3]),
            &text[..text.len() - 3],
            "char removed whole → still valid, nothing salvaged away"
        );
        assert!(std::str::from_utf8(&text[..text.len() - 1]).is_err());
        assert_eq!(
            trim_partial_tail(&text[..text.len() - 1]),
            &text[..text.len() - 3]
        );
        assert_eq!(
            trim_partial_tail(&text[..text.len() - 2]),
            &text[..text.len() - 3]
        );
    }

    #[test]
    fn sanitize_keeps_everyday_text_including_arabic() {
        assert_eq!(
            sanitize_terminal("حزمة إلغاء 3️⃣ \t Indicates\nok"),
            "حزمة إلغاء 3️⃣ \t Indicates\nok",
            "ordinary text, tabs and newlines are byte-identical"
        );
    }

    #[test]
    fn sanitize_removes_escapes_clipboard_and_reorder_markup() {
        // OSC52 clipboard hijack + ANSI moves + an RLO reordering attempt.
        // Sanitization neutralizes the control/framework bytes; the sequence
        // payloads ("]52;c;…", "[0K") are plain text and render inertly.
        let hostile = "\x1b]52;c;Q0FMTA\x07حز reasonably\x1b[0K\u{202e}we\u{200e}ird\tX";
        assert_eq!(
            sanitize_terminal(hostile),
            "]52;c;Q0FMTAحز reasonably[0Kweird\tX",
            "ESC/BEL/override/LRM bytes are gone; text + tab remain; the payload cannot regenerate a sequence without its control bytes"
        );
        // C1 controls and DEL.
        assert_eq!(sanitize_terminal("a\u{009b}b\u{7f}c"), "abc");
        // CR is dropped (LF/tabs kept): rendering stays line-safe.
        assert_eq!(sanitize_terminal("a\rb\nc\t"), "ab\nc\t");
        // LRM/RLM/isolates.
        assert_eq!(sanitize_terminal("\u{200f}\u{2066}x\u{2069}"), "x");
    }

    #[test]
    fn error_snippet_is_capped_sanitized_and_bounded() {
        let corrupt = "\u{1b}]0;Great\x07hello".repeat(300);
        let s = error_snippet(&corrupt, 500);
        assert!(!s.contains('\u{1b}'), "no ESC survives the snippet");
        assert!(s.contains("hello"), "real text survives");
        // Byte cap 500 on ASCII stays ASCII-sized; on multibyte a lossy map
        // can widen per char but stays a small constant factor — bounded.
        let mut wide = String::new();
        for _ in 0..600 {
            wide.push('ح');
        }
        assert!(error_snippet(&wide, 500).chars().count() <= 500);
    }
}
