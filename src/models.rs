//! Simulation-model / scenario / example inventory across the DES repos.
//! Knows where each repo actually keeps runnable models:
//!   - discrete-event-system.rs: cargo examples (examples/*.rs), the ~142
//!     demo bins (src/bin/*.rs), and committed JSON scenario data (data/).
//!   - des-engine: the 95 JSON model specs (examples/*.json) run via
//!     `npm run from-json examples/<file>.json`, plus example.lp.
//!   - old-outmoded-engine.rs: legacy soccer demo bins (src/bin/*.rs).

use std::path::Path;

/// Where model/scenario files live: (repo, label, subdir, extension,
/// recursive, how-to-run hint).
pub const MODEL_RULES: &[(&str, &str, &str, &str, bool, &str)] = &[
    (
        "discrete-event-system.rs",
        "cargo examples",
        "examples",
        "rs",
        false,
        "cargo run --example <name>",
    ),
    (
        "discrete-event-system.rs",
        "demo bins",
        "src/bin",
        "rs",
        false,
        "cargo run --bin <name>",
    ),
    (
        "discrete-event-system.rs",
        "scenario data (JSON)",
        "data",
        "json",
        true,
        "loaded by the demo bins",
    ),
    (
        "des-engine",
        "JSON model specs",
        "examples",
        "json",
        false,
        "npm run from-json examples/<file>.json",
    ),
    (
        "des-engine",
        "LP samples",
        ".",
        "lp",
        false,
        "LP-format input",
    ),
    (
        "old-outmoded-engine.rs",
        "legacy soccer demo bins",
        "src/bin",
        "rs",
        false,
        "legacy — canonical soccer engine is in ~/codes/akrion-sim",
    ),
];

const SKIP_DIRS: &[&str] = &["node_modules", "target", "out", "dist", ".git", "tmp"];

fn list_files(dir: &Path, ext: &str, recursive: bool, acc: &mut Vec<String>, prefix: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut items: Vec<_> = entries.flatten().collect();
    items.sort_by_key(|e| e.file_name());
    for entry in items {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if path.is_dir() {
            if recursive && !SKIP_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                list_files(&path, ext, true, acc, &format!("{prefix}{name}/"));
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            acc.push(format!("{prefix}{name}"));
        }
    }
}

/// Build the inventory for one repo (or all repos when `repo` is None),
/// optionally filtering file names by a substring.
pub fn inventory(root: &Path, repo: Option<&str>, filter: Option<&str>) -> String {
    let mut out = String::new();
    let mut total = 0usize;
    for (rule_repo, label, subdir, ext, recursive, hint) in MODEL_RULES {
        if let Some(want) = repo
            && want != *rule_repo
        {
            continue;
        }
        let dir = root.join(rule_repo).join(subdir);
        if !dir.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        list_files(&dir, ext, *recursive, &mut files, "");
        if let Some(f) = filter {
            let f = f.to_lowercase();
            files.retain(|name| name.to_lowercase().contains(&f));
        }
        if files.is_empty() {
            continue;
        }
        total += files.len();
        out.push_str(&format!(
            "## {rule_repo} — {label} ({} file(s) in {subdir}/, run: {hint})\n",
            files.len()
        ));
        const SHOW: usize = 100;
        for name in files.iter().take(SHOW) {
            out.push_str(&format!("  {subdir}/{name}\n"));
        }
        if files.len() > SHOW {
            out.push_str(&format!("  …[{} more]\n", files.len() - SHOW));
        }
        out.push('\n');
    }
    if total == 0 {
        out.push_str("no model/example files matched.\n");
    }
    out.push_str(
        "(the `des` repo is uta-phd research material — browse it directly; \
         JSON specs for des_engine citizens are mostly generated at runtime \
         from each citizen's example_spec)\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("des-models-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let eng = root.join("des-engine/examples");
        std::fs::create_dir_all(&eng).unwrap();
        std::fs::write(eng.join("tiger-pomdp.json"), "{}").unwrap();
        std::fs::write(eng.join("blackjack-mc.json"), "{}").unwrap();
        std::fs::write(eng.join("notes.md"), "not a model").unwrap();
        let rs = root.join("discrete-event-system.rs/src/bin");
        std::fs::create_dir_all(&rs).unwrap();
        std::fs::write(rs.join("main_elevator.rs"), "fn main() {}").unwrap();
        let data = root.join("discrete-event-system.rs/data/soccer/policies");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("team-policy.snapshot.json"), "{}").unwrap();
        // junk that must be skipped in recursive walks
        let junk = root.join("discrete-event-system.rs/data/node_modules");
        std::fs::create_dir_all(&junk).unwrap();
        std::fs::write(junk.join("dep.json"), "{}").unwrap();
        root
    }

    #[test]
    fn inventory_finds_models_and_skips_junk() {
        let root = temp_root("all");
        let out = inventory(&root, None, None);
        assert!(out.contains("examples/tiger-pomdp.json"));
        assert!(out.contains("src/bin/main_elevator.rs"));
        assert!(out.contains("data/soccer/policies/team-policy.snapshot.json"));
        assert!(!out.contains("notes.md"));
        assert!(!out.contains("node_modules"));
        assert!(out.contains("npm run from-json"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn inventory_filters_by_repo_and_substring() {
        let root = temp_root("filter");
        let only_ts = inventory(&root, Some("des-engine"), None);
        assert!(only_ts.contains("tiger-pomdp.json"));
        assert!(!only_ts.contains("main_elevator.rs"));
        let filtered = inventory(&root, None, Some("tiger"));
        assert!(filtered.contains("tiger-pomdp.json"));
        assert!(!filtered.contains("blackjack"));
        let none = inventory(&root, None, Some("zzz-no-such"));
        assert!(none.contains("no model/example files matched"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
