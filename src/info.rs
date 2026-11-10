//! v0.10.3 Phase B: `--info PKG` — resolve ONE exact package name across
//! all four sources, first hit wins:
//! 1. installed (localdb via `orphans::package_details` — full dependency
//!    details: depends/required_by/optional_for/reason)
//! 2. sync DBs (`search_repo` substring scan + client-side exact filter)
//! 3. AUR (blocking `/search?by=name` + client-side exact filter)
//! 4. Flatpak (appstream search + app-id leaf/exact filter)
//!
//! Every source fails open — a source error just skips that source, the
//! same contract the search paths have. Nothing anywhere is an Err so the
//! CLI exits 1 with one clear stderr line. All display strings keep the
//! v0.9.3 sanitize_terminal contract at the print boundary
//! (`output::print_info`); the `--json` path stays byte-identical by design.
//!
//! Note (recorded honestly): the plan drafted "AUR exact via fetch_aur_info"
//! but that helper is async and versions-only (built for update checks);
//! the blocking by-name search carries every display field `Package` maps,
//! so it is the correct seam here.

use crate::config::Config;
use crate::model::Package;

/// Case-insensitive exact-name match with a flatpak twist: reverse-DNS
/// app-IDs (`org.mozilla.firefox`) also match their last dot-segment
/// (`firefox`), which is what users actually type. Dotless names fall
/// through unchanged, so repo/AUR matching is unaffected.
fn is_exact(pkg_name: &str, query: &str) -> bool {
    if pkg_name.eq_ignore_ascii_case(query) {
        return true;
    }
    pkg_name
        .rsplit('.')
        .next()
        .is_some_and(|leaf| leaf.eq_ignore_ascii_case(query))
}

pub fn resolve_info(name: &str, cfg: &Config) -> anyhow::Result<Package> {
    let query = name.trim();
    if query.is_empty() {
        anyhow::bail!("no package named '' — provide a package name");
    }

    // 1. Installed: alpm localdb is exact-name already; init failures
    //    (no pacman.conf, no libalpm) just skip the source.
    if let Some(pkg) = crate::orphans::package_details(query) {
        return Ok(pkg);
    }

    // 2. Sync DBs: the substring scan walks every syncdb entry anyway, so
    //    pass limit 0 (no truncation) and filter exact names here — a
    //    truncated list could drop the exact match (e.g. query "z" matching
    //    "az…" before "zardus").
    if let Ok(pkgs) = crate::search::search_repo(query, 0, false, false)
        && let Some(pkg) = pkgs.iter().find(|p| is_exact(&p.name, query))
    {
        return Ok(pkg.clone());
    }

    // 3. AUR: popularity-sorted results could truncate the exact match away
    //    (e.g. substring neighbors with more votes), so again limit 0.
    if let Ok(pkgs) = crate::search::search_aur_blocking_with_config(query, "name", 0, false, cfg)
        && let Some(pkg) = pkgs.iter().find(|p| is_exact(&p.name, query))
    {
        return Ok(pkg.clone());
    }

    // 4. Flatpak: `search_flatpak_with_config` is fail-open by contract
    //    (disabled, binary missing, timeout → Ok(empty)).
    if let Ok(pkgs) = crate::sources::search_flatpak_with_config(query, 0, false, cfg)
        && let Some(pkg) = pkgs.iter().find(|p| is_exact(&p.name, query))
    {
        return Ok(pkg.clone());
    }

    anyhow::bail!("no package named '{query}' in repo/AUR/flatpak")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_name_is_case_insensitive() {
        assert!(is_exact("pacman", "PACMAN"));
        assert!(is_exact("Pacman", "pacman"));
        assert!(!is_exact("pacman-contrib", "pacman"));
    }

    #[test]
    fn flatpak_leaf_match() {
        assert!(is_exact("org.mozilla.firefox", "firefox"));
        assert!(is_exact("org.mozilla.firefox", "FIREFOX"));
        assert!(!is_exact("org.mozilla.firefox", "moz"));
        assert!(is_exact("pacman", "pacman"));
    }
}
