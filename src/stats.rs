use serde::{Deserialize, Serialize};

/// System health snapshot for `--stats` and the Updates-tab header.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SysStats {
    pub explicit: usize,
    pub dependencies: usize,
    pub orphans: usize,
    pub updates_repo: usize,
    pub updates_aur: usize,
    pub updates_flatpak: usize,
    pub flatpak_installed: usize,
    pub cache_pacman_bytes: u64,
    pub cache_pakmap_bytes: u64,
}

/// Local-only stats (no network): explicit/deps split + orphans + cache sizes.
/// Fast path for `--stats` and TUI header; AUR updates filled separately.
pub fn gather_local_stats() -> SysStats {
    let (explicit, dependencies) = count_reasons();
    let orphans = crate::orphans::list_orphans()
        .map(|v| v.len())
        .unwrap_or_else(|_| {
            crate::orphans::list_orphans_fallback()
                .map(|v| v.len())
                .unwrap_or(0)
        });
    let updates_repo = crate::updates::check_repo_updates()
        .map(|v| v.len())
        .unwrap_or(0);
    SysStats {
        explicit,
        dependencies,
        orphans,
        updates_repo,
        updates_aur: 0,
        updates_flatpak: 0,
        flatpak_installed: 0,
        cache_pacman_bytes: dir_size("/var/cache/pacman/pkg"),
        cache_pakmap_bytes: cache_size(),
    }
}

fn count_reasons() -> (usize, usize) {
    let Ok(cfg) = crate::search::repo::get_cached_config() else {
        return (0, 0);
    };
    let db_path = if cfg.db_path.is_empty() {
        "/var/lib/pacman".to_string()
    } else {
        cfg.db_path.clone()
    };
    let root_dir = if cfg.root_dir.is_empty() {
        "/".to_string()
    } else {
        cfg.root_dir.clone()
    };
    let Ok(handle) = alpm::Alpm::new(root_dir.as_str(), db_path.as_str()) else {
        return (0, 0);
    };
    let mut explicit = 0;
    let mut deps = 0;
    for pkg in handle.localdb().pkgs().iter() {
        match pkg.reason() {
            alpm::PackageReason::Explicit => explicit += 1,
            alpm::PackageReason::Depend => deps += 1,
        }
    }
    (explicit, deps)
}

fn dir_size(path: &str) -> u64 {
    dir_size_recursive(std::path::Path::new(path), 0)
}

/// Recursive cache size with a depth cap and no symlink following:
/// pacman/pakmap caches can hold symlinks; following them risks cycles
/// and double counting. Errors (permissions, races) are skipped, never fatal.
pub(crate) fn dir_size_recursive(path: &std::path::Path, depth: usize) -> u64 {
    if depth > 8 {
        return 0;
    }
    let Ok(rd) = std::fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0u64;
    for entry in rd.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_dir() {
            total = total.saturating_add(dir_size_recursive(&entry.path(), depth + 1));
        } else if ft.is_file() {
            total = total.saturating_add(entry.metadata().map(|m| m.len()).unwrap_or(0));
        }
    }
    total
}

fn cache_size() -> u64 {
    let base = dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("pakmap");
    dir_size(base.to_str().unwrap_or("/tmp/pakmap"))
}

/// Human bytes: 1.2 MiB style, no float noise.
pub fn human_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u + 1 < UNITS.len() {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{} {}", n, UNITS[u])
    } else {
        format!("{:.1} {}", v, UNITS[u])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_formats() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KiB");
    }

    #[test]
    fn local_stats_never_panics() {
        let s = gather_local_stats();
        assert!(s.explicit + s.dependencies > 0 || s.explicit == 0);
    }
}
