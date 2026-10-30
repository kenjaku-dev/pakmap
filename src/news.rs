use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// One Arch news entry (RSS subset). `needs_action` = title/summary mentions
/// manual intervention in a form the upgrade guard should surface.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NewsItem {
    pub title: String,
    pub link: String,
    pub pub_date: Option<String>,
    pub pub_epoch: Option<i64>,
    pub needs_action: bool,
}

const FEED_URL: &str = "https://archlinux.org/feeds/news/";
const INTERVENTION_HINTS: &[&str] = &[
    "manual intervention",
    "action required",
    "requires intervention",
    "requires manual",
];

/// v0.9.3 (Phase 3): the feed (and every cached copy of it) is untrusted
/// input. Titles are filtered to display-safe characters, and each link must
/// pass the shared external-URL gate — one policy with the GUI opener. An
/// item with a hostile link stays listed, but its link is blanked: display,
/// copy and open all read the same string, so there is no second path that
/// can resurrect the URL. pakmap itself never fetches these links (the user's
/// browser does), and a blank link disables the GUI's open/copy buttons.
fn purge_untrusted(mut items: Vec<NewsItem>) -> Vec<NewsItem> {
    for item in &mut items {
        item.title = crate::safety::text::sanitize_terminal(&item.title);
        if crate::safety::url::validate_external_url(&item.link).is_err() {
            item.link = String::new();
        }
    }
    items
}

fn cache_file(cfg: &crate::config::Config) -> Option<PathBuf> {
    // v0.9.3: the cache base is verified (owned, 0700, no symlink, trusted
    // parent) and the HOME-less fallback is per-UID instead of the shared
    // /tmp/pakmap. On rejection the news flow degrades the documented way:
    // live fetch without a cache, never an error.
    match crate::safety::fs::ensure_cache_base(cfg) {
        Ok(base) => Some(base.join("arch-news.json")),
        Err(e) => {
            tracing::warn!(target: "pakmap::news", "news cache dir rejected: {e}");
            None
        }
    }
}

fn cache_fresh(path: &std::path::Path, ttl_secs: u64) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let Ok(mtime) = meta.modified() else {
        return false;
    };
    let Ok(age) = SystemTime::now().duration_since(mtime) else {
        return false;
    };
    age < Duration::from_secs(ttl_secs.max(60))
}

/// News cache JSON holds nothing but RSS titles — a file beyond this cap is
/// corrupt/hostile and is dropped (the live fetch follows on this path).
const NEWS_CACHE_CAP_BYTES: usize = 1024 * 1024;

fn load_cache(path: &std::path::Path) -> Option<Vec<NewsItem>> {
    let bounded = crate::safety::fs::read_text_bounded(path, NEWS_CACHE_CAP_BYTES).ok()?;
    // Stale caches predate Phase 3's at-source filtering — purge again so
    // every read surface (CLI guard, TUI, GUI get_news) sees the same shape.
    serde_json::from_str(&bounded.text)
        .ok()
        .map(purge_untrusted)
}

fn save_cache(path: &std::path::Path, items: &[NewsItem]) {
    if let Ok(s) = serde_json::to_string(items)
        && let Err(e) = crate::safety::fs::atomic_write(path, s.as_bytes())
    {
        // Atomic rename (v0.9.3): a mid-write crash can no longer leave a
        // truncated JSON. A failed write just loses this refresh's cache.
        tracing::warn!(target: "pakmap::news", "news cache write failed: {e}");
    }
}

/// Fetch Arch news with file cache (`caching` skill: cache-aside, TTL + jitter
/// via news_cache_secs). Offline/errors -> cached-or-empty, never Err-fatal
/// (`resilience-failure`: fail-open, degraded empty list).
pub fn fetch_arch_news(cfg: &crate::config::Config) -> Vec<NewsItem> {
    if !cfg.news_enabled() {
        return vec![];
    }
    let ttl = cfg.behavior.news_cache_secs.max(60);
    if let Some(path) = cache_file(cfg) {
        if cache_fresh(&path, ttl)
            && let Some(items) = load_cache(&path)
        {
            return items;
        }
        let fresh = fetch_live(cfg);
        if !fresh.is_empty() {
            save_cache(&path, &fresh);
            return fresh;
        }
        // Live failed: serve stale cache if any (degraded, explicit).
        if let Some(stale) = load_cache(&path) {
            return stale;
        }
        return vec![];
    }
    fetch_live(cfg)
}

fn fetch_live(cfg: &crate::config::Config) -> Vec<NewsItem> {
    let timeout = Duration::from_secs(cfg.search.timeout_secs.clamp(3, 30).min(10));
    let client = reqwest::blocking::Client::builder()
        .user_agent(format!("pakmap/{}", env!("CARGO_PKG_VERSION")))
        .timeout(timeout)
        .build();
    let Ok(client) = client else {
        return vec![];
    };
    let Ok(resp) = client.get(FEED_URL).send() else {
        return vec![];
    };
    if !resp.status().is_success() {
        return vec![];
    }
    let Ok(text) =
        crate::safety::net::read_body_capped(resp, crate::safety::net::FEED_BODY_CAP_BYTES)
    else {
        return vec![];
    };
    parse_rss(&text)
}

/// Minimal RSS parse (no extra deps): item blocks -> title/link/pubDate/description.
pub fn parse_rss(xml: &str) -> Vec<NewsItem> {
    let mut out = Vec::new();
    for block in xml.split("<item>").skip(1) {
        let end = block.find("</item>").unwrap_or(block.len());
        let item = &block[..end];
        let title = tag_text(item, "title").unwrap_or_default();
        let link = tag_text(item, "link").unwrap_or_default();
        let pub_date = tag_text(item, "pubDate");
        let desc = tag_text(item, "description").unwrap_or_default();
        if title.trim().is_empty() {
            continue;
        }
        let hay = format!("{title} {desc}").to_lowercase();
        let needs_action = INTERVENTION_HINTS.iter().any(|h| hay.contains(h));
        let pub_epoch = pub_date.as_deref().and_then(parse_rfc2822_epoch);
        out.push(NewsItem {
            title: html_unescape(&title),
            link,
            pub_date: pub_date.clone(),
            pub_epoch,
            needs_action,
        });
        if out.len() >= 30 {
            break;
        }
    }
    purge_untrusted(out)
}

fn tag_text(block: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let s = block.find(&open)? + open.len();
    let e = block[s..].find(&close)? + s;
    Some(block[s..e].trim().to_string())
}

fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("<![CDATA[", "")
        .replace("]]>", "")
        .trim()
        .to_string()
}

/// True when any item needs manual action (guard before `pacman -Syu`).
/// `recent_only`: when pubDate parses and is older than 7d, ignore it.
/// Unparseable dates count as recent (fail-closed for safety).
pub fn has_intervention(items: &[NewsItem], recent_only: bool) -> bool {
    items.iter().any(|i| {
        if !i.needs_action {
            return false;
        }
        if !recent_only {
            return true;
        }
        let epoch = i
            .pub_epoch
            .or_else(|| i.pub_date.as_deref().and_then(parse_rfc2822_epoch));
        match epoch {
            Some(e) => {
                let now = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                now - e < 7 * 86400
            }
            // Unparseable dates count as recent (fail-closed for safety).
            None => true,
        }
    })
}

/// Parse RFC2822 dates as emitted by the Arch news feed, e.g.
/// `Mon, 01 Jan 2026 00:00:00 +0000`, to unix epoch. Hand-rolled std-only
/// (no chrono dep for one feed): returns None on garbage, and the guard
/// treats None as recent (fail-closed for safety).
pub fn parse_rfc2822_epoch(s: &str) -> Option<i64> {
    // Strip optional weekday prefix.
    let s = s.trim();
    let s = match s.split_once(", ") {
        Some((_, rest)) => rest.trim(),
        None => s,
    };
    // Expect: DD Mon YYYY HH:MM:SS ±ZZZZ
    let mut parts = s.split_whitespace();
    let day: i64 = parts.next()?.parse().ok()?;
    let month: i64 = match parts.next()? {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let year: i64 = parts.next()?.parse().ok()?;
    let time = parts.next()?;
    let mut t = time.split(':');
    let (hh, mm, ss): (i64, i64, i64) = (
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
    );
    if t.next().is_some()
        || !(0..24).contains(&hh)
        || !(0..60).contains(&mm)
        || !(0..61).contains(&ss)
    {
        return None;
    }
    let tz = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let tz_secs = parse_tz_offset(tz)?;
    // Days from civil (Howard Hinnant) → epoch days.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (month + 9).rem_euclid(12);
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + hh * 3600 + mm * 60 + ss - tz_secs)
}

fn parse_tz_offset(tz: &str) -> Option<i64> {
    // Numeric ±HHMM, or classic military/zone names seen in feeds.
    if tz.eq_ignore_ascii_case("UT")
        || tz.eq_ignore_ascii_case("GMT")
        || tz.eq_ignore_ascii_case("Z")
    {
        return Some(0);
    }
    let (sign, digits) = match tz.strip_prefix('+') {
        Some(d) => (1i64, d),
        None => {
            let d = tz.strip_prefix('-')?;
            (-1i64, d)
        }
    };
    if digits.len() != 4 {
        return None;
    }
    let hh: i64 = digits[..2].parse().ok()?;
    let mm: i64 = digits[2..].parse().ok()?;
    if hh > 14 || mm > 59 {
        return None;
    }
    Some(sign * (hh * 3600 + mm * 60))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0"?>
<rss><channel>
<item><title>Linux 6.9 requires manual intervention</title><link>https://archlinux.org/news/1</link><pubDate>Mon, 01 Jan 2026 00:00:00 +0000</pubDate><description>Action required for mkinitcpio</description></item>
<item><title>Routine mirror update</title><link>https://archlinux.org/news/2</link><pubDate>Mon, 01 Jan 2026 00:00:00 +0000</pubDate><description>Nothing to do</description></item>
</channel></rss>"#;

    #[test]
    fn parse_detects_intervention() {
        let items = parse_rss(SAMPLE);
        assert_eq!(items.len(), 2);
        assert!(items[0].needs_action);
        assert!(!items[1].needs_action);
        assert!(has_intervention(&items, false));
    }

    #[test]
    fn empty_feed_is_no_guard() {
        assert!(!has_intervention(&[], false));
        assert!(parse_rss("<rss></rss>").is_empty());
    }

    #[test]
    fn disabled_config_returns_empty() {
        let mut cfg = crate::config::Config::default();
        cfg.behavior.news_enabled = false;
        assert!(fetch_arch_news(&cfg).is_empty());
    }

    // v0.9.3 Phase 3: hostile feed contents can't reach any display surface.
    #[test]
    fn hostile_titles_are_sanitized_and_unsafe_links_blanked() {
        let feed = concat!(
            "<rss><channel>",
            "<item><title>Real news</title>",
            "<link>https://archlinux.org/news/x</link></item>",
            // ESC + RLO + ZWJ: sequence payload chars stay, control bytes go.
            "<item><title>\x1b[31M\u{202e}fake\u{200d}line</title>",
            "<link>javascript:alert(1)</link></item>",
            // file:// and data: schemes are worth own vectors (GUI open gate).
            "<item><title>Another</title><link>file:///etc/passwd</link></item>",
            "<item><title>Data one</title><link>data:text/plain,hi</link></item>",
            "</channel></rss>"
        );
        let items = parse_rss(feed);
        assert_eq!(items.len(), 4);
        // Safe link survives intact.
        assert_eq!(items[0].link, "https://archlinux.org/news/x");
        // Hostile titles lose controls/bidi (payload text remains visible).
        assert_eq!(items[1].title, "[31Mfakeline");
        // Every unsafe scheme is blanked, never dropped — the item itself
        // (title/date/action flag) stays listed.
        for item in &items[1..] {
            assert_eq!(item.link, "", "{:?} must be blanked", item.title);
        }
        // Real chars survive: Arabic title keeps its letters.
        let arabic = parse_rss(
            "<item><title>تحديث النواة</title><link>https://archlinux.org/news/a</link></item>",
        );
        assert_eq!(arabic[0].title, "تحديث النواة");
    }

    #[test]
    fn cached_items_repurge_on_load() {
        // A cache written before Phase 3 may contain raw hostile content.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("arch-news.json");
        let stale = r#"[{"title":"\u001b[31hostile","link":"javascript:alert(1)",
            "pub_date":null,"pub_epoch":null,"needs_action":true}]"#;
        std::fs::write(&p, stale).unwrap();
        let items = load_cache(&p).unwrap();
        assert_eq!(items.len(), 1);
        assert!(!items[0].title.contains('\u{1b}'));
        assert_eq!(items[0].link, "");

        // A well-formed cache round-trips untouched.
        let good = r#"[{"title":"ok","link":"https://archlinux.org/news/z",
            "pub_date":null,"pub_epoch":null,"needs_action":false}]"#;
        std::fs::write(&p, good).unwrap();
        assert_eq!(
            load_cache(&p).unwrap()[0].link,
            "https://archlinux.org/news/z"
        );
    }

    #[test]
    fn rfc2822_vectors() {
        // Vectors verified with `date -u -d ... +%s` on the build machine.
        assert_eq!(
            parse_rfc2822_epoch("Mon, 01 Jan 2026 00:00:00 +0000"),
            Some(1767225600)
        );
        assert_eq!(
            parse_rfc2822_epoch("Thu, 01 Jan 1970 00:00:00 +0000"),
            Some(0)
        );
        assert_eq!(
            parse_rfc2822_epoch("Tue, 15 Aug 2023 12:30:00 +0200"),
            Some(1692095400)
        );
        // Same instant, different zone rendering.
        assert_eq!(
            parse_rfc2822_epoch("Tue, 15 Aug 2023 10:30:00 +0000"),
            Some(1692095400)
        );
        // Garbage never parses (guard treats as recent = fail-closed).
        assert_eq!(parse_rfc2822_epoch("not a date"), None);
        assert_eq!(parse_rfc2822_epoch("32 Foo 2026 99:99:99 +0000"), None);
        assert_eq!(parse_rfc2822_epoch(""), None);
    }
}
