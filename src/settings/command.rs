//! `pakmap config …` subcommand behavior (plan §2). The enum lives in
//! `cli.rs`; this module executes the operations. Every output ends the
//! process with conventional codes: 0 ok, 1 refusal/conflict/io/unparseable,
//! 2 usage error (unknown key, invalid value).

use std::io::IsTerminal as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::cli::{ConfigCmd, ConfigTarget};
use crate::config::Config;
use crate::settings::catalog::{CATALOG, ENV_ROWS, Kind, SettingMeta, section_of};
use crate::settings::validate::{nearest_key, numeric_clamp, validate as validate_value};
use crate::settings::{BOTTOM_UP_ALIAS, Resolved, origin::Origin, resolve, write};

fn env_fn() -> impl Fn(&str) -> Option<String> {
    |k: &str| std::env::var(k).ok()
}

fn project_trusted() -> bool {
    std::env::var_os("PAKMAP_TRUST_PROJECT").is_some_and(|v| v == "1")
}

fn user_path(cli_cfg: Option<&Path>) -> Option<PathBuf> {
    cli_cfg.map(Path::to_path_buf).or_else(Config::default_path)
}

fn project_path() -> PathBuf {
    Config::project_path().unwrap_or_else(|| {
        std::env::current_dir()
            .map(|cwd| cwd.join("pakmap.toml"))
            .unwrap_or_else(|_| PathBuf::from("pakmap.toml"))
    })
}

fn clap_target(
    cli_cfg: Option<&Path>,
    which: ConfigTarget,
    yes: bool,
) -> Result<write::Target, &'static str> {
    Ok(match which {
        ConfigTarget::User => {
            let path = user_path(cli_cfg).ok_or("no config directory")?;
            let create = yes || path.exists();
            write::Target::User { path, create }
        }
        ConfigTarget::Project => write::Target::Project {
            path: project_path(),
            trusted: project_trusted(),
        },
    })
}

/// Dispatch one `config` subcommand; returns the process exit code.
pub fn run(cmd: &ConfigCmd, cli_cfg: Option<&Path>) -> i32 {
    match cmd {
        ConfigCmd::List {
            json,
            origin,
            section,
        } => list(*json, *origin, section.as_deref(), cli_cfg),
        ConfigCmd::Get { key } => get(key, cli_cfg),
        ConfigCmd::Set {
            key,
            value,
            target,
            yes,
        } => set(key, value, *target, *yes, cli_cfg),
        ConfigCmd::Unset { key, target } => unset(key, *target, cli_cfg),
        ConfigCmd::Validate { json, strict } => validate(*json, *strict, cli_cfg),
        ConfigCmd::Path => config_path(cli_cfg),
        ConfigCmd::Docs => {
            print!("{}", Config::example_toml());
            0
        }
    }
}

/// Engine resolve for the CLI commands; engine warnings surface on stderr
/// (one line each, warn-only — same channel as `Config::load_resolved`).
fn resolved(cli_cfg: Option<&Path>) -> Resolved {
    let r = resolve(cli_cfg, &env_fn());
    for w in &r.warnings {
        eprintln!("{w}");
    }
    r
}

fn origin_for(r: &Resolved, key: &str) -> Origin {
    r.origins
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, o)| o.clone())
        .unwrap_or_else(|| {
            // D1: an untouched gui.theme is decided by the device itself.
            if key == "gui.theme" {
                Origin::Device
            } else {
                Origin::Default
            }
        })
}

/// One display row of `config list` (plan §2: KEY VALUE ORIGIN APPLY DANGER).
struct Row {
    key: String,
    value: String,
    origin: Origin,
    apply: &'static str,
    danger: &'static str,
    group: &'static str,
    desc: String,
}

fn list(json: bool, expand_origin: bool, section: Option<&str>, cli_cfg: Option<&Path>) -> i32 {
    let r = resolved(cli_cfg);
    let filter = section
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase);

    let mut rows: Vec<Row> = Vec::new();
    for meta in CATALOG {
        if filter
            .as_ref()
            .is_some_and(|f| section_of(meta.key) != f.as_str())
        {
            continue;
        }
        rows.push(Row {
            key: meta.key.to_string(),
            value: meta.field.get(&r.config),
            origin: origin_for(&r, meta.key),
            apply: meta.apply.as_str(),
            danger: meta.danger.as_str(),
            group: meta.group.as_label(),
            desc: meta.desc.to_string(),
        });
    }
    // Env knobs are read-only info rows; they show with no filter or under
    // `--section env` (plan §1.1 EnvOnly).
    if filter.as_deref().is_none_or(|f| f == "env") {
        for row in ENV_ROWS {
            rows.push(Row {
                key: row.name.to_string(),
                value: std::env::var(row.name).unwrap_or_else(|_| "(unset)".into()),
                origin: Origin::Env,
                apply: "immediate",
                danger: "-",
                group: "Environment",
                desc: row.desc.to_string(),
            });
        }
    }

    if json {
        let items: Vec<serde_json::Value> = rows
            .iter()
            .map(|row| {
                let path = match row.origin {
                    Origin::UserFile(ref p) | Origin::ProjectFile(ref p) => {
                        Some(p.display().to_string())
                    }
                    _ => None,
                };
                serde_json::json!({
                    "key": row.key,
                    "value": row.value,
                    "group": row.group,
                    "origin": row.origin.as_label(),
                    "origin_path": path,
                    "apply": row.apply,
                    "danger": row.danger,
                    "desc": row.desc,
                })
            })
            .collect();
        let doc = serde_json::to_string_pretty(&items).unwrap_or_else(|_| "[]".into());
        println!("{doc}");
        return 0;
    }

    let header = ["KEY", "VALUE", "ORIGIN", "APPLY", "DANGER"];
    println!(
        "{:<24} {:<20} {:<16} {:<13} {}",
        header[0], header[1], header[2], header[3], header[4]
    );
    for row in &rows {
        let origin_col = row.origin.render(expand_origin);
        let origin_wide: String = if expand_origin {
            origin_col.clone()
        } else {
            truncate(&origin_col, 16)
        };
        println!(
            "{:<24} {:<20} {:<16} {:<13} {}",
            truncate(&row.key, 24),
            truncate(&row.value, 20),
            origin_wide,
            row.apply,
            if row.danger.is_empty() {
                "-"
            } else {
                row.danger
            }
        );
    }
    0
}

/// Char-count truncation with an ellipsis — column budget only; JSON keeps
/// full values.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn get(key: &str, cli_cfg: Option<&Path>) -> i32 {
    if key == BOTTOM_UP_ALIAS {
        // D4-adjacent: the alias is still readable, resolved onto its
        // canonical key with a migration note.
        let r = resolved(cli_cfg);
        eprintln!("note: [behavior] bottom_up is a deprecated alias of [search] bottom_up");
        println!("{}", r.config.search.bottom_up);
        return 0;
    }
    match SettingMeta::lookup(key) {
        Some(meta) => {
            let r = resolved(cli_cfg);
            println!("{}", meta.field.get(&r.config));
            0
        }
        None => {
            eprintln!("{}", unknown_key_message(key));
            2
        }
    }
}

fn unknown_key_message(key: &str) -> String {
    let hint = nearest_key(key)
        .map(|n| format!(" — did you mean '{n}'?"))
        .unwrap_or_default();
    format!("unknown setting key '{key}'{hint}; run `pakmap config docs` for all keys")
}

fn set(key: &str, value: &str, which: ConfigTarget, yes: bool, cli_cfg: Option<&Path>) -> i32 {
    let target = match clap_target(cli_cfg, which, yes) {
        Ok(t) => t,
        Err(msg) => {
            eprintln!("settings set: {msg}");
            return 1;
        }
    };
    match consent_to_create(&target) {
        Some(t) => finish_set(key, value, &t),
        None => 1, // declined at the prompt; message already printed
    }
}

/// §1.6: a missing user file is created only with explicit consent — `--yes`
/// non-interactively (already folded into `create` by `clap_target`), or a
/// y/N prompt on a TTY. A `None` return means the user declined; anything
/// else falls through (the engine reports refusals with a create hint when
/// the file is absent).
fn consent_to_create(target: &write::Target) -> Option<write::Target> {
    let write::Target::User { path, create } = target else {
        return Some(target.clone()); // project files are never created (D2)
    };
    if *create || path.exists() || !std::io::stdin().is_terminal() {
        return Some(target.clone());
    }
    print!(
        "Create {} from the documented example template? [y/N] ",
        path.display()
    );
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    let answer = line.trim();
    if answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes") {
        Some(write::Target::User {
            path: path.clone(),
            create: true,
        })
    } else {
        eprintln!("cancelled");
        None
    }
}

fn finish_set(key: &str, value: &str, target: &write::Target) -> i32 {
    match write::set(key, value, target) {
        Ok(out) => {
            if out.created {
                eprintln!(
                    "Created {} from the documented example template",
                    out.path.display()
                );
            }
            println!("Set {key} = {value} in {}", out.path.display());
            for w in &out.warnings {
                eprintln!("{w}");
            }
            0
        }
        Err(e) => {
            eprintln!("settings set: {}", e.message());
            exit_code(&e)
        }
    }
}

fn unset(key: &str, target: ConfigTarget, cli_cfg: Option<&Path>) -> i32 {
    let target = match clap_target(cli_cfg, target, false) {
        Ok(t) => t,
        Err(msg) => {
            eprintln!("settings unset: {msg}");
            return 1;
        }
    };
    match write::unset(key, &target) {
        Ok(out) => {
            for w in &out.warnings {
                eprintln!("{w}");
            }
            println!("Unset {key} in {} (back to default)", out.path.display());
            0
        }
        Err(e) => {
            eprintln!("settings unset: {}", e.message());
            exit_code(&e)
        }
    }
}

fn exit_code(e: &write::SetError) -> i32 {
    match e {
        write::SetError::UnknownKey(_) | write::SetError::InvalidValue(_) => 2,
        _ => 1,
    }
}

fn config_path(cli_cfg: Option<&Path>) -> i32 {
    match user_path(cli_cfg) {
        Some(p) => println!("{}", p.display()),
        None => println!("(no config dir)"),
    }
    if let Some(p) = Config::project_path() {
        println!("project: {}", p.display());
    }
    0
}

fn validate(json: bool, strict: bool, cli_cfg: Option<&Path>) -> i32 {
    let mut findings: Vec<serde_json::Value> = Vec::new();
    let mut human: Vec<String> = Vec::new();
    if let Some(p) = user_path(cli_cfg)
        && p.exists()
    {
        findings_for_file(&p, &mut human, &mut findings);
    }
    if let Some(p) = Config::project_path() {
        findings_for_file(&p, &mut human, &mut findings);
    }
    if json {
        let doc = serde_json::json!({ "findings": findings, "count": findings.len() });
        println!(
            "{}",
            serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".into())
        );
    } else if findings.is_empty() {
        println!("OK: no findings");
    } else {
        for line in &human {
            println!("{line}");
        }
        println!("{} findings", findings.len());
    }
    if strict && !findings.is_empty() { 1 } else { 0 }
}

/// Lexical findings for one file: unknown keys (nearest-key suggestion),
/// invalid values (write-door rules), the deprecated alias, unparseable and
/// io states — all per plan §2, fail-open (never fatal to the tool).
fn findings_for_file(path: &Path, human: &mut Vec<String>, findings: &mut Vec<serde_json::Value>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        let msg = format!("warn: cannot read {}", path.display());
        human.push(msg.clone());
        findings.push(serde_json::json!({
            "kind": "io", "key": null, "path": path.display().to_string(),
            "line": null, "message": msg, "suggestion": null
        }));
        return;
    };
    if let Err(e) = toml::from_str::<Config>(&text) {
        let msg = format!("invalid config {}: {e}", path.display());
        human.push(msg.clone());
        findings.push(serde_json::json!({
            "kind": "unparseable", "key": null, "path": path.display().to_string(),
            "line": null, "message": msg, "suggestion": null
        }));
        return;
    }
    let display = path.display().to_string();
    for found in crate::settings::scanner::scan(&text).keys {
        let at = format!("{}:{} ", display, found.line_no);
        if found.key == BOTTOM_UP_ALIAS {
            let msg =
                format!("{at}[behavior] bottom_up is a deprecated alias of [search] bottom_up");
            human.push(msg.clone());
            findings.push(serde_json::json!({
                "kind": "alias", "key": found.key, "path": display,
                "line": found.line_no, "message": msg, "suggestion": "search.bottom_up"
            }));
            continue;
        }
        match SettingMeta::lookup(&found.key) {
            Some(meta) => {
                if let Some(raw) = raw_value_at(&text, found.line_no)
                    && let Err(reason) = validate_value(meta, &raw)
                {
                    let msg = format!("{at}'{}' = {raw:?} — {reason}", found.key);
                    human.push(msg.clone());
                    findings.push(serde_json::json!({
                        "kind": "invalid-value", "key": found.key, "path": display,
                        "line": found.line_no, "message": msg, "suggestion": null
                    }));
                }
            }
            None => {
                let sugg = nearest_key(&found.key);
                let tail = sugg
                    .map(|s| format!("; did you mean '{s}'?"))
                    .unwrap_or_default();
                let msg = format!("{at}'{}' is not a pakmap setting{tail}", found.key);
                human.push(msg.clone());
                findings.push(serde_json::json!({
                    "kind": "unknown-key", "key": found.key, "path": display,
                    "line": found.line_no, "message": msg, "suggestion": sugg
                }));
            }
        }
    }
}

/// Pull the raw value text of a `key =` line for reporting (first line of a
/// multi-line value). TOML-quote aware enough for messages: quoted strings
/// return their inner raw text; other scalars drop trailing comments.
fn raw_value_at(text: &str, line_no: usize) -> Option<String> {
    let line = text.lines().nth(line_no.checked_sub(1)?)?;
    let (_, rest) = line.split_once('=')?;
    let t = rest.trim();
    for (q, close) in [('"', '"'), ('\'', '\'')] {
        if let Some(inner) = t.strip_prefix(q)
            && let Some(end) = inner.find(close)
        {
            return Some(inner[..end].to_string());
        }
    }
    let cut = t.find('#').unwrap_or(t.len());
    let out = t[..cut].trim();
    if out.is_empty() {
        return None;
    }
    Some(
        out.trim_start_matches('[')
            .trim_end_matches(']')
            .to_string(),
    )
}

// ---------------------------------------------------------------------------
// Generated settings reference (plan §5): docs/settings.md
// ---------------------------------------------------------------------------

/// Render one markdown cell safely (pipes/newlines are structural).
fn md_cell(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('\n', " ")
}

/// Human kind label with the inline constraints the engine enforces:
/// numeric clamps (`numeric_clamp`) and CATALOG-ordered enum values.
fn kind_label(kind: Kind, key: &str) -> String {
    match kind {
        Kind::Bool => "bool".to_string(),
        Kind::U16 | Kind::U32 | Kind::U64 | Kind::Usize => {
            if let Some((min, max)) = numeric_clamp(key) {
                format!("number {min}..={max}")
            } else {
                "number".to_string()
            }
        }
        Kind::Enum(vals) => format!(
            "enum({})",
            vals.iter().map(|v| v.name).collect::<Vec<_>>().join("/")
        ),
        Kind::Style => "style string".to_string(),
        Kind::Text => "text".to_string(),
        Kind::OptText => "text, blank = unset".to_string(),
        Kind::PairU16 => "pair of two numbers (cols,rows), e.g. 70,24".to_string(),
    }
}

/// One config truth table row of the generated reference.
fn doc_row(meta: &SettingMeta) -> String {
    let default = if meta.default.is_empty() {
        "(unset)".to_string()
    } else {
        md_cell(meta.default)
    };
    let danger = match meta.danger.as_str() {
        "" => "-".to_string(),
        other => other.to_string(),
    };
    format!(
        "| `{}` | {} | {} | `{}` | {} | {} | {} |\n",
        meta.key,
        meta.group.as_label(),
        md_cell(&kind_label(meta.kind, meta.key)),
        default,
        meta.apply.as_str(),
        danger,
        md_cell(meta.desc)
    )
}

/// Full generated reference for `docs/settings.md` (plan §5): the same
/// catalog truth behind `config docs`/`--init-config` plus a per-key table,
/// env knobs, and the write policy. Deterministic — the checked-in copy is
/// pinned byte-for-byte by a drift-gate unit test in this module.
pub fn settings_doc() -> String {
    let mut out = String::new();
    out.push_str(
        "<!-- generated by src/settings/command.rs::settings_doc — edit the\n\
         settings catalog (src/settings/catalog.rs), never this file; a\n\
         drift-gate unit test pins the checked-in copy byte-for-byte.\n\
         Regenerate: PAKMAP_REGEN_DOCS=1 cargo test --lib settings::command -->\n\n",
    );
    out.push_str("# pakmap settings reference\n\n");
    out.push_str(
        "Every reader of pakmap meets the same configuration and the same \
         write rules. One engine (`src/settings/`) resolves defaults < user \
         file < project file, then serves three doors over one catalog:\n\n\
         - **CLI** — `pakmap config {list,get,set,unset,validate,path,docs}`\n\
         - **TUI** — press `S` (settings modal): filter, Space toggles, \
         Enter edits, `r` resets a row, `s` saves, `e` opens `$EDITOR`\n\
         - **GUI** — the Settings screen (gear button or `s`)\n\n\
         All three render origins, apply tags, and danger rows from the \
         tables below; the CLI and the GUI write through the same engine, \
         the TUI writes the user file only.\n\n",
    );
    out.push_str("## Example config (generated)\n\n```toml\n");
    out.push_str(&Config::example_toml());
    out.push_str("```\n\n");
    out.push_str(&format!(
        "## Keys ({} settings + {} env knobs)\n\n{}\n",
        CATALOG.len(),
        ENV_ROWS.len(),
        "| Key | Group | Kind | Default | Apply | Danger | Description |\n\
         |---|---|---|---|---|---|---|"
    ));
    for meta in CATALOG {
        out.push_str(&doc_row(meta));
    }
    out.push_str(
        "\nApply: `immediate` takes effect live where the app \
         supports it (the TUI/GUI live-apply `[theme]` keys; `no_color` \
         is one-way — a file can never re-enable color after `--no-color` \
         or `NO_COLOR`), `next-launch` keys apply on relaunch. Env knobs \
         are read-only and apply at process spawn.\n\n\
         ## Environment variables\n\n\
         | Variable | Description |\n|---|---|\n",
    );
    for env in ENV_ROWS {
        out.push_str(&format!("| `{}` | {} |\n", env.name, md_cell(env.desc)));
    }
    out.push_str(
        "\n## Write policy\n\n\
         - `set`/`unset` write the **user file** by default \
         (`~/.config/pakmap/config.toml`, or `--config <path>` for this \
         session); `--target project` writes `./pakmap.toml` — which is \
         only honored at load for the `search`/`tui`/`theme` sections \
         unless the project layer is trusted (`PAKMAP_TRUST_PROJECT=1`), and project files are never created (only edited in place).\n\
         - Nothing is created silently: the user file is created only \
         with `--yes` (or the GUI/TUI affordances that ask).\n\
         - Writes are atomic (temp + rename), preserve comments, \
         spacing, and inline `# notes` (lexical line surgery on the \
         target line only), and are guarded by a fingerprint conflict \
         gate that refuses when the file changed on disk meanwhile.\n\
         - Value validation is the engine's: per-kind parsing, enum \
         membership in the table order, and numeric clamps — refusals \
         same as the table's constraints.\n\
         - `[behavior] bottom_up` is a deprecated alias of \
         `[search] bottom_up`: reads still resolve (the same-file \
         canonical spelling wins), writes are refused with the \
         migration hint.\n\
         - Exit codes: `0` ok · `1` refused (policy/conflict/io/\
         unparseable) · `2` usage (unknown key, invalid value).\n",
    );
    out
}

#[cfg(test)]
mod docs_tests {
    use super::*;

    /// plan §5 drift gate: `docs/settings.md` equals the catalog-generated
    /// reference byte-for-byte. Regenerate with
    /// `PAKMAP_REGEN_DOCS=1 cargo test --lib settings::command`.
    #[test]
    fn docs_settings_md_matches_the_catalog() {
        let generated = settings_doc();
        if std::env::var_os("PAKMAP_REGEN_DOCS").is_some() {
            std::fs::write(
                concat!(env!("CARGO_MANIFEST_DIR"), "/docs/settings.md"),
                &generated,
            )
            .unwrap();
            println!("regenerated docs/settings.md from the current catalog");
        }
        let on_disk = include_str!("../../docs/settings.md");
        if generated != on_disk {
            let first_diff = generated
                .lines()
                .zip(on_disk.lines())
                .position(|(a, b)| a != b)
                .unwrap_or_else(|| generated.lines().count().min(on_disk.lines().count()));
            panic!(
                "docs/settings.md drifted from the catalog at line {} — \
                 regenerate: PAKMAP_REGEN_DOCS=1 cargo test --lib settings::command",
                first_diff + 1
            );
        }
    }
}
