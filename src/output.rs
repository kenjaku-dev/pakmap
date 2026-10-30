use colored::Colorize;

use crate::cli::Source;
use crate::model::Package;

pub fn print_packages(
    repo_packages: &[Package],
    aur_packages: &[Package],
    flatpak_packages: &[Package],
    source: Source,
    bottom_up: bool,
    json: bool,
    no_color: bool,
) -> anyhow::Result<()> {
    if json {
        let mut all = Vec::new();
        if matches!(source, Source::Repo | Source::All) {
            all.extend_from_slice(repo_packages);
        }
        if matches!(source, Source::Aur | Source::All) {
            all.extend_from_slice(aur_packages);
        }
        if matches!(source, Source::Flatpak | Source::All) {
            all.extend_from_slice(flatpak_packages);
        }
        println!("{}", serde_json::to_string_pretty(&all)?);
        return Ok(());
    }

    if no_color {
        colored::control::set_override(false);
    }

    // Order handling
    let mut sections: Vec<(&str, &[Package])> = vec![];
    if bottom_up {
        // Flatpak first (reverse of default)
        if matches!(source, Source::Flatpak | Source::All) {
            sections.push(("flatpak", flatpak_packages));
        }
        if matches!(source, Source::Aur | Source::All) {
            sections.push(("aur", aur_packages));
        }
        if matches!(source, Source::Repo | Source::All) {
            sections.push(("repo", repo_packages));
        }
    } else {
        // Repo first (default like pacman), flatpak last
        if matches!(source, Source::Repo | Source::All) {
            sections.push(("repo", repo_packages));
        }
        if matches!(source, Source::Aur | Source::All) {
            sections.push(("aur", aur_packages));
        }
        if matches!(source, Source::Flatpak | Source::All) {
            sections.push(("flatpak", flatpak_packages));
        }
    }

    let mut total = 0usize;
    for (_kind, pkgs) in &sections {
        total += pkgs.len();
    }

    if total == 0 {
        eprintln!("{}", "No packages found.".yellow());
        return Ok(());
    }

    for (kind, pkgs) in sections {
        for pkg in pkgs {
            print_package(pkg, kind);
        }
    }

    eprintln!(
        "\n{} {} (repo: {}, aur: {}, flatpak: {})",
        "Found".dimmed(),
        total.to_string().bold(),
        repo_packages.len(),
        aur_packages.len(),
        flatpak_packages.len()
    );

    Ok(())
}

fn print_package(pkg: &Package, kind: &str) {
    // Format: repo/name version [installed] (votes/pop) - desc
    // v0.9.3 (CWE-150): AUR/RPC strings reach a real terminal here — filter
    // to display-safe characters at the print boundary. The `--json` path
    // (send to scripts) stays byte-identical by design.
    let repo_part = format!(
        "{}/{}",
        crate::safety::text::sanitize_terminal(&pkg.repo),
        crate::safety::text::sanitize_terminal(&pkg.name)
    );
    let repo_colored = if kind == "aur" {
        repo_part.bright_magenta().bold()
    } else if kind == "flatpak" {
        repo_part.bright_cyan().bold()
    } else {
        match pkg.repo.as_str() {
            "core" => repo_part.red().bold(),
            "extra" => repo_part.green().bold(),
            "multilib" => repo_part.blue().bold(),
            "local" => repo_part.green().bold(),
            "system" | "world" | "galaxy" | "lib32" => repo_part.cyan().bold(),
            _ => repo_part.yellow().bold(),
        }
    };

    let version = crate::safety::text::sanitize_terminal(&pkg.version).white();
    let installed = if pkg.installed {
        " [installed]".bright_green().bold().to_string()
    } else {
        "".to_string()
    };

    let aur_extra = if pkg.repo == "aur" {
        let votes = pkg.votes.unwrap_or(0);
        let pop = pkg.popularity.unwrap_or(0.0);
        let ood = pkg
            .out_of_date
            .map(|_| " [OOD]".red().bold().to_string())
            .unwrap_or_default();
        let orphan = if pkg.orphan {
            " [ORPHAN]".red().to_string()
        } else {
            String::new()
        };
        let unmaint = if pkg
            .maintainer
            .as_deref()
            .map(|m| m.trim().is_empty())
            .unwrap_or(true)
        {
            " [UNMAINTAINED]".yellow().to_string()
        } else {
            String::new()
        };
        format!(" (+{} {:.2}){}{}{}", votes, pop, ood, orphan, unmaint)
    } else {
        if pkg.orphan {
            " [ORPHAN]".red().to_string()
        } else {
            String::new()
        }
    };

    let desc = crate::safety::text::sanitize_terminal(pkg.short_desc()).dimmed();

    println!("{} {} {}{}", repo_colored, version, aur_extra, installed);
    println!("    {}", desc);
    // 0.5.0: dependency hints for safety (remove path) — one line, dimmed.
    if let Some(req) = pkg.required_by.as_ref().filter(|v| !v.is_empty()) {
        let preview: Vec<&str> = req.iter().take(5).map(|s| s.as_str()).collect();
        let more = if req.len() > 5 {
            format!(" +{}", req.len() - 5)
        } else {
            String::new()
        };
        println!(
            "    {}",
            format!("Required by: {}{}", preview.join(", "), more).dimmed()
        );
    }
    if let Some(opt) = pkg.optdepends.as_ref().filter(|v| !v.is_empty()) {
        let preview: Vec<&str> = opt.iter().take(3).map(|s| s.as_str()).collect();
        println!(
            "    {}",
            format!("Optdepends: {}", preview.join(", ")).dimmed()
        );
    }
}

pub fn print_updates(
    updates: &[crate::updates::PkgUpdate],
    json: bool,
    no_color: bool,
) -> anyhow::Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(updates)?);
        return Ok(());
    }
    if no_color {
        colored::control::set_override(false);
    }
    if updates.is_empty() {
        eprintln!("{}", "System up to date.".green());
        return Ok(());
    }
    for u in updates {
        let name = format!("{}/{}", u.repo, u.name).bold();
        let arrow = format!(
            "{} -> {}",
            u.old_version.dimmed(),
            u.new_version.green().bold()
        );
        let ood = if u.out_of_date {
            " [OOD]".red().bold().to_string()
        } else {
            String::new()
        };
        println!("{}{} {}", name, ood, arrow);
    }
    eprintln!(
        "\n{} {}",
        "Updates:".dimmed(),
        updates.len().to_string().bold()
    );
    Ok(())
}

pub fn print_orphans(pkgs: &[Package], json: bool, no_color: bool) -> anyhow::Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(pkgs)?);
        return Ok(());
    }
    if no_color {
        colored::control::set_override(false);
    }
    if pkgs.is_empty() {
        eprintln!("{}", "No orphans found.".green());
        return Ok(());
    }
    for pkg in pkgs {
        println!(
            "{} {}",
            format!("local/{}", pkg.name).bold(),
            pkg.version.dimmed()
        );
    }
    eprintln!(
        "\n{} {}",
        "Orphans:".dimmed(),
        pkgs.len().to_string().bold()
    );
    eprintln!(
        "{}",
        "Remove with: pakmap --remove <pkg>  (or Tab remover in TUI)".dimmed()
    );
    Ok(())
}

pub fn print_stats(
    stats: &crate::stats::SysStats,
    json: bool,
    no_color: bool,
) -> anyhow::Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(stats)?);
        return Ok(());
    }
    if no_color {
        colored::control::set_override(false);
    }
    println!("{} {}", "Explicit:".bold(), stats.explicit);
    println!("{} {}", "Dependencies:".bold(), stats.dependencies);
    println!(
        "{} {}",
        "Orphans:".bold(),
        if stats.orphans == 0 {
            stats.orphans.to_string().green().bold().to_string()
        } else {
            stats.orphans.to_string().yellow().bold().to_string()
        }
    );
    println!("{} {}", "Repo updates:".bold(), stats.updates_repo);
    if stats.updates_aur > 0 {
        println!("{} {}", "AUR updates:".bold(), stats.updates_aur);
    }
    if stats.updates_flatpak > 0 {
        println!("{} {}", "Flatpak updates:".bold(), stats.updates_flatpak);
    }
    if stats.flatpak_installed > 0 {
        println!("{} {}", "Flatpak apps:".bold(), stats.flatpak_installed);
    }
    println!(
        "{} {}",
        "Pacman cache:".bold(),
        crate::stats::human_bytes(stats.cache_pacman_bytes).dimmed()
    );
    println!(
        "{} {}",
        "pakmap cache:".bold(),
        crate::stats::human_bytes(stats.cache_pakmap_bytes).dimmed()
    );
    Ok(())
}

pub fn print_json_error(msg: &str) {
    let v = serde_json::json!({"error": msg});
    let s =
        serde_json::to_string_pretty(&v).unwrap_or_else(|_| format!(r#"{{"error":"{}"}}"#, msg));
    eprintln!("{}", s);
}

pub fn print_clean(
    preview: &crate::install::clean::CleanPreview,
    dry_run: bool,
    json: bool,
    no_color: bool,
) -> anyhow::Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(preview)?);
        return Ok(());
    }
    if no_color {
        colored::control::set_override(false);
    }
    if preview.files == 0 {
        eprintln!("{}", "Cache already clean.".green());
        return Ok(());
    }
    let size = crate::stats::human_bytes(preview.bytes);
    if dry_run {
        println!(
            "Would remove {} files ({}) [keep {}]",
            preview.files,
            size.green().bold(),
            preview.keep,
        );
        eprintln!(
            "{}",
            "Preview only — nothing removed. Drop --dry-run to clean.".dimmed()
        );
    } else {
        println!(
            "{} {} files ({})",
            "✓ Cleaned".green().bold(),
            preview.files,
            size.green().bold(),
        );
    }
    Ok(())
}
