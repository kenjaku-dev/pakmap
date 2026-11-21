//! `pakmap appimage …` CLI dispatch — formatting and exit codes only.
//! The actual work lives in `crate::appimage::service`, shared with the GUI.

use std::io::{IsTerminal as _, Write as _};

use crate::appimage::service;
use crate::cli::AppImageCmd;

fn fail(code: &str, message: impl std::fmt::Display, hint: Option<&str>) -> i32 {
    eprintln!("[appimage] ERROR {code}: {message}");
    if let Some(h) = hint {
        eprintln!("[appimage] hint: {h}");
    }
    1
}

fn confirm_remove(id: &str, assume_yes: bool) -> bool {
    if assume_yes || !std::io::stdin().is_terminal() {
        return true;
    }
    print!("[appimage] remove '{id}' and all files it created? [y/N] ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    line.trim().eq_ignore_ascii_case("y")
}

/// Dispatch one `appimage` subcommand; returns the process exit code.
pub fn run(cmd: &AppImageCmd) -> i32 {
    match cmd {
        AppImageCmd::Integrate { path, name } => match service::integrate(path, name.as_deref()) {
            Ok(entry) => {
                println!("Integrated {} ({})", entry.name, entry.id);
                println!("  sha256: {}", entry.sha256);
                println!("Run 'pakmap appimage list' to see managed files.");
                0
            }
            Err(e) => fail("appimage_integrate_failed", &e, None),
        },
        AppImageCmd::Install {
            url,
            sha256,
            allow_unverified,
            name,
        } => match service::install(url, sha256.as_deref(), *allow_unverified, name.as_deref()) {
            Ok(installed) => {
                println!(
                    "Installed {} ({}) from {}",
                    installed.entry.name, installed.entry.id, url
                );
                println!("  sha256: {}", installed.entry.sha256);
                println!("  downloaded: {} bytes", installed.downloaded_bytes);
                0
            }
            Err(e) => fail("appimage_install_failed", &e, None),
        },
        AppImageCmd::List { json } => match service::list() {
            Ok(entries) => {
                if *json {
                    match serde_json::to_string_pretty(&entries) {
                        Ok(s) => println!("{s}"),
                        Err(e) => return fail("appimage_list_failed", &e, None),
                    }
                } else if entries.is_empty() {
                    println!("No integrated AppImages.");
                } else {
                    for e in &entries {
                        println!("{:<24} {}  {}", e.id, e.name, e.sha256);
                    }
                }
                0
            }
            Err(e) => fail("appimage_list_failed", &e, None),
        },
        AppImageCmd::Remove { id, yes } => {
            if !confirm_remove(id, *yes) {
                println!("Cancelled.");
                return 0;
            }
            match service::remove(id) {
                Ok(()) => {
                    println!("Removed {id}.");
                    0
                }
                Err(e) => fail("appimage_remove_failed", &e, None),
            }
        }
    }
}
