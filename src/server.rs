//! The MCP tool surface. Thin wrappers over util/models/org_map/supabase/
//! github/domains/cloudflare so the logic stays unit-testable outside the
//! MCP plumbing. All tools are read-only or build-only.

use std::path::PathBuf;
use std::time::Duration;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::util::{
    self, DES_REPOS, git, parse_failing_tests, parse_test_summaries, run_cmd, safe_pg_filter_value,
    safe_segment, truncate_output,
};
use crate::{cloudflare, domains, github, models, org_map, supabase};

#[derive(Deserialize, JsonSchema)]
pub struct RepoReq {
    /// DES repo directory name under the org root, e.g.
    /// "discrete-event-system.rs". See org_overview for the list.
    repo: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct RecentCommitsReq {
    /// DES repo directory name under the org root.
    repo: String,
    /// How many commits to show (default 15).
    count: Option<u32>,
    /// Branch or ref to log (default: current HEAD).
    branch: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SearchCodeReq {
    /// Extended regex passed to `git grep -E`. Searches tracked files only,
    /// so target/, node_modules/, out/ and dist/ are never scanned.
    pattern: String,
    /// Restrict to one DES repo (directory name). Default: all DES repos.
    repo: Option<String>,
    /// Cap on total matching lines returned (default 200).
    max_matches: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CargoCheckReq {
    /// Rust DES repo (default "discrete-event-system.rs"). The legacy
    /// "old-outmoded-engine.rs" also works (read-only reference).
    repo: Option<String>,
    /// Also check tests/benches/examples (`--all-targets`). Default false —
    /// note the current engine has ~142 demo bins, so this is slow.
    all_targets: Option<bool>,
    /// Timeout in seconds (default 600).
    timeout_secs: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct EngineTestsReq {
    /// Rust DES repo (default "discrete-event-system.rs"). For
    /// "old-outmoded-engine.rs" the org convention is lib-only tests.
    repo: Option<String>,
    /// Optional cargo test name filter (substring match).
    filter: Option<String>,
    /// Run only library tests (`cargo test --lib`). Defaults to true for
    /// old-outmoded-engine.rs, false otherwise.
    lib_only: Option<bool>,
    /// Run the full matrix the engine CI uses
    /// (`--all-targets --all-features`). Default false.
    all_targets: Option<bool>,
    /// Timeout in seconds (default 900).
    timeout_secs: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SimModelsReq {
    /// Restrict to one DES repo (directory name). Default: all.
    repo: Option<String>,
    /// Substring filter on file names, e.g. "pomdp" or "elevator".
    filter: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct DnsLookupReq {
    /// Domain name to resolve.
    domain: String,
    /// Record type (A, AAAA, CNAME, MX, TXT, NS, …). Default: a sweep of
    /// A, AAAA, CNAME, NS, MX, TXT.
    record_type: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct DomainInfoReq {
    /// Registrable domain (not a subdomain).
    domain: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct TlsCertReq {
    /// Hostname to check.
    host: String,
    /// Port (default 443).
    port: Option<u16>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CfRecordsReq {
    /// Zone name.
    zone: String,
    /// Filter by record type (A, CNAME, TXT, …).
    record_type: Option<String>,
    /// Filter by exact record name.
    name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CiStatusReq {
    /// Workflow runs to show per repo (default 1, max 10).
    per_repo: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct LogSessionsReq {
    /// Filter by environment (production / staging / development / unknown).
    environment: Option<String>,
    /// Filter by user id (uuid).
    user_id: Option<String>,
    /// Max sessions to return (default 20, max 100).
    limit: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct TailLogsReq {
    /// Session id from client_log_sessions.
    session_id: String,
    /// Filter by level: error, warn, info, debug, trace.
    level: Option<String>,
    /// Max entries (default 100, max 500).
    limit: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ErrorSummaryReq {
    /// Look-back window in hours (default 24, max 720).
    hours: Option<u32>,
    /// Filter by environment.
    environment: Option<String>,
    /// Group the error/warn entries by this field instead of message:
    /// one of message (default), url, sim_id, user_id, session_id, level.
    /// Use url/sim_id to localize a failure to a route or model.
    group_by: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ClientLogTraceReq {
    /// Session id from client_log_sessions.
    session_id: String,
    /// Max entries (default 300, max 1000).
    limit: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SimModelInspectReq {
    /// DES repo the spec lives in (default "des-engine", which holds the 95
    /// JSON model specs under examples/).
    repo: Option<String>,
    /// Path to the JSON spec RELATIVE to the repo root, e.g.
    /// "examples/blackjack-mc.json". No absolute paths, no "..".
    file: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct StackStatusReq {
    /// Also probe this apex domain's A record (e.g. the org's marketing/app
    /// host). Optional — omit to skip the DNS check.
    domain: Option<String>,
    /// Timeout (seconds) for the engine build check (default 300, max 1200).
    timeout_secs: Option<u64>,
}

const LOG_LEVELS: &[&str] = &["error", "warn", "info", "debug", "trace"];

#[derive(Clone)]
pub struct DesMcp {
    pub root: PathBuf,
    tool_router: ToolRouter<Self>,
}

impl DesMcp {
    pub fn new() -> Self {
        Self {
            root: util::org_root(),
            tool_router: Self::tool_router(),
        }
    }

    /// Resolve a DES repo name (allowlist + on-disk check). The org root
    /// (~/codes/ores by default) holds many NON-DES repos, so only the
    /// repos in util::DES_REPOS are addressable.
    fn repo_path(&self, name: &str) -> Result<PathBuf, String> {
        safe_segment(name, "repo")?;
        if !DES_REPOS.contains(&name) {
            return Err(format!(
                "{name:?} is not a DES repo — this server only manages {DES_REPOS:?} \
                 (the org root also holds unrelated repos)"
            ));
        }
        let p = self.root.join(name);
        if !p.is_dir() {
            return Err(format!(
                "no such repo dir {:?} under {} — use org_overview to see what exists",
                name,
                self.root.display()
            ));
        }
        Ok(p)
    }

    fn rust_repo_path(&self, name: &str) -> Result<PathBuf, String> {
        let p = self.repo_path(name)?;
        if !p.join("Cargo.toml").is_file() {
            return Err(format!(
                "{name:?} has no Cargo.toml — cargo tools only apply to the Rust \
                 engine repos (discrete-event-system.rs, old-outmoded-engine.rs)"
            ));
        }
        Ok(p)
    }

    /// The DES repos that exist under the org root.
    fn present_repos(&self) -> Vec<&'static str> {
        DES_REPOS
            .iter()
            .copied()
            .filter(|name| self.root.join(name).is_dir())
            .collect()
    }
}

impl Default for DesMcp {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router]
impl DesMcp {
    // ------------------------------------------------------------- org / git

    #[tool(
        description = "Overview of the DES repos under the org root (DES_ROOT, default ~/codes/ores): branch, dirty file count, ahead/behind upstream, last commit. Only the DES allowlist is shown — the root also holds unrelated repos. Start here to orient."
    )]
    async fn org_overview(&self) -> Result<String, String> {
        let repos = self.present_repos();
        if repos.is_empty() {
            return Err(format!(
                "none of the DES repos {DES_REPOS:?} exist under {} — set DES_ROOT",
                self.root.display()
            ));
        }
        let mut out = format!("org root: {} (DES repos only)\n\n", self.root.display());
        for name in &repos {
            let dir = self.root.join(name);
            if !dir.join(".git").exists() {
                out.push_str(&format!(
                    "## {name}\n  (not a git repo — research material)\n\n"
                ));
                continue;
            }
            let branch = git(&dir, &["rev-parse", "--abbrev-ref", "HEAD"])
                .await
                .map(|(_, s)| s.trim().to_string())
                .unwrap_or_else(|e| format!("<{e}>"));
            let dirty = git(&dir, &["status", "--porcelain"])
                .await
                .map(|(_, s)| s.lines().count())
                .unwrap_or(0);
            let last = git(&dir, &["log", "-1", "--format=%h %ad %s", "--date=short"])
                .await
                .map(|(_, s)| s.trim().to_string())
                .unwrap_or_default();
            let upstream = match git(
                &dir,
                &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"],
            )
            .await
            {
                Ok((true, s)) => {
                    let mut it = s.split_whitespace();
                    let behind = it.next().unwrap_or("0");
                    let ahead = it.next().unwrap_or("0");
                    format!("ahead {ahead}, behind {behind}")
                }
                _ => "no upstream".to_string(),
            };
            out.push_str(&format!(
                "## {name}\n  branch: {branch} ({upstream})\n  dirty files: {dirty}\n  last commit: {last}\n\n"
            ));
        }
        out.push_str(
            "(repos will migrate into github.com/discrete-event-systems over time — see org_map)\n",
        );
        Ok(truncate_output(out))
    }

    #[tool(
        description = "Detailed git status for one DES repo: branch, upstream delta, short status, recent commits, worktrees, and stashes."
    )]
    async fn repo_status(&self, Parameters(req): Parameters<RepoReq>) -> Result<String, String> {
        let dir = self.repo_path(&req.repo)?;
        if !dir.join(".git").exists() {
            return Err(format!("{:?} is not a git repo", req.repo));
        }
        let mut out = String::new();
        for (title, args) in [
            ("branch", vec!["rev-parse", "--abbrev-ref", "HEAD"]),
            ("status", vec!["status", "--short", "--branch"]),
            (
                "recent commits",
                vec!["log", "-8", "--format=%h %ad %an %s", "--date=short"],
            ),
            ("worktrees", vec!["worktree", "list"]),
            ("stashes", vec!["stash", "list"]),
        ] {
            let (_, text) = git(&dir, &args).await?;
            let text = text.trim();
            if !text.is_empty() {
                out.push_str(&format!("## {title}\n{text}\n\n"));
            }
        }
        Ok(truncate_output(out))
    }

    #[tool(description = "Show recent commits for a DES repo (git log).")]
    async fn recent_commits(
        &self,
        Parameters(req): Parameters<RecentCommitsReq>,
    ) -> Result<String, String> {
        let dir = self.repo_path(&req.repo)?;
        let n = req.count.unwrap_or(15).min(200).to_string();
        let mut args = vec![
            "log",
            "--format=%h %ad %an %s",
            "--date=short",
            "-n",
            n.as_str(),
        ];
        if let Some(branch) = req.branch.as_deref() {
            util::safe_token(branch, "branch")?;
            args.push(branch);
        }
        let (ok, text) = git(&dir, &args).await?;
        if !ok {
            return Err(text);
        }
        Ok(truncate_output(text))
    }

    #[tool(
        description = "Search tracked source across the DES repos (or one repo) with `git grep -E`. Use this instead of raw grep — it skips target/, node_modules/, out/, dist/."
    )]
    async fn search_code(
        &self,
        Parameters(req): Parameters<SearchCodeReq>,
    ) -> Result<String, String> {
        let repos: Vec<String> = match req.repo.as_deref() {
            Some(r) => {
                self.repo_path(r)?;
                vec![r.to_string()]
            }
            None => self.present_repos().iter().map(|s| s.to_string()).collect(),
        };
        let max = req.max_matches.unwrap_or(200) as usize;
        let mut lines_out: Vec<String> = Vec::new();
        let mut hit_repos = 0usize;
        for name in &repos {
            if lines_out.len() >= max {
                break;
            }
            let dir = self.root.join(name);
            if !dir.join(".git").exists() {
                continue;
            }
            let (ok, text) = run_cmd(
                Some(&dir),
                "git",
                &["grep", "-nEI", "-e", &req.pattern, "--", ":!*.lock"],
                Duration::from_secs(60),
            )
            .await?;
            if !ok {
                continue; // exit 1 = no matches in this repo
            }
            hit_repos += 1;
            for l in text.lines() {
                if lines_out.len() >= max {
                    lines_out.push(format!("…[capped at {max} matches]"));
                    break;
                }
                lines_out.push(format!("{name}/{l}"));
            }
        }
        if lines_out.is_empty() {
            return Ok(format!(
                "no matches for {:?} in {} repo(s)",
                req.pattern,
                repos.len()
            ));
        }
        Ok(truncate_output(format!(
            "matches in {hit_repos} repo(s):\n{}",
            lines_out.join("\n")
        )))
    }

    // ---------------------------------------------------------------- builds

    #[tool(
        description = "Fast compile check (`cargo check`) for a Rust engine repo (default discrete-event-system.rs); returns error/warning lines only. Build-only, never mutates."
    )]
    async fn cargo_check(
        &self,
        Parameters(req): Parameters<CargoCheckReq>,
    ) -> Result<String, String> {
        let repo = req.repo.as_deref().unwrap_or("discrete-event-system.rs");
        let dir = self.rust_repo_path(repo)?;
        let mut args = vec!["check"];
        if req.all_targets.unwrap_or(false) {
            args.push("--all-targets");
        }
        let timeout = Duration::from_secs(req.timeout_secs.unwrap_or(600).min(3600));
        let (ok, text) = run_cmd(Some(&dir), "cargo", &args, timeout).await?;
        let interesting: Vec<&str> = text
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                t.starts_with("error") || t.starts_with("warning") || t.starts_with("-->")
            })
            .collect();
        let mut out = format!(
            "cargo {} in {}: {}\n",
            args.join(" "),
            repo,
            if ok { "OK" } else { "FAILED" }
        );
        if !interesting.is_empty() {
            out.push('\n');
            out.push_str(&interesting.join("\n"));
        }
        Ok(truncate_output(out))
    }

    #[tool(
        description = "Run `cargo test` for a Rust engine repo (default discrete-event-system.rs; engine CI matrix is all_targets=true i.e. `--all-targets --all-features`). For old-outmoded-engine.rs, lib_only defaults to true (the legacy convention is `cargo test --lib`). Returns the failing-test set plus per-target summaries."
    )]
    async fn engine_tests(
        &self,
        Parameters(req): Parameters<EngineTestsReq>,
    ) -> Result<String, String> {
        let repo = req.repo.as_deref().unwrap_or("discrete-event-system.rs");
        let dir = self.rust_repo_path(repo)?;
        let lib_only = req.lib_only.unwrap_or(repo == "old-outmoded-engine.rs");
        let mut args: Vec<&str> = vec!["test"];
        if lib_only {
            args.push("--lib");
        } else if req.all_targets.unwrap_or(false) {
            args.extend(["--all-targets", "--all-features"]);
        }
        if let Some(f) = req.filter.as_deref() {
            util::safe_token(f, "test filter")?;
            args.push(f);
        }
        let timeout = Duration::from_secs(req.timeout_secs.unwrap_or(900).min(3600));
        let (_, text) = run_cmd(Some(&dir), "cargo", &args, timeout).await?;

        let failing = parse_failing_tests(&text);
        let summaries = parse_test_summaries(&text);
        let mut out = format!(
            "cargo {} in {}\nfailing tests: {}\n\n",
            args.join(" "),
            repo,
            failing.len()
        );
        if !failing.is_empty() {
            let shown = failing.len().min(300);
            out.push_str("## failing\n");
            out.push_str(&failing[..shown].join("\n"));
            if shown < failing.len() {
                out.push_str(&format!("\n…[{} more]", failing.len() - shown));
            }
            out.push_str("\n\n");
        }
        out.push_str("## per-target summaries\n");
        out.push_str(&summaries.join("\n"));
        Ok(truncate_output(out))
    }

    #[tool(
        description = "Type-check the ORIGINAL TypeScript engine (des-engine, package uta-phd-des) with `npx tsc -p tsconfig.json --noEmit`. Compile-check only; per-domain tests remain `npm run test-*` scripts run manually."
    )]
    async fn ts_engine_check(&self) -> Result<String, String> {
        let dir = self.repo_path("des-engine")?;
        if !dir.join("tsconfig.json").is_file() {
            return Err("des-engine has no tsconfig.json — is the checkout complete?".to_string());
        }
        let (ok, text) = run_cmd(
            Some(&dir),
            "npx",
            &["tsc", "-p", "tsconfig.json", "--noEmit"],
            Duration::from_secs(600),
        )
        .await?;
        let mut out = format!(
            "tsc --noEmit in des-engine: {}\n",
            if ok { "OK" } else { "FAILED" }
        );
        let errors: Vec<&str> = text
            .lines()
            .filter(|l| l.contains("error TS") || l.trim_start().starts_with("Found "))
            .collect();
        if !errors.is_empty() {
            out.push('\n');
            out.push_str(&errors.join("\n"));
        }
        Ok(truncate_output(out))
    }

    // ------------------------------------------------------- models & docs

    #[tool(
        description = "Inventory of simulation models / scenarios / examples across the DES repos: des-engine's 95 JSON model specs (examples/*.json, run via `npm run from-json`), discrete-event-system.rs cargo examples and ~142 demo bins (src/bin/main_*.rs) plus committed data/, and the legacy soccer bins. Optional repo + substring filter."
    )]
    async fn sim_models(
        &self,
        Parameters(req): Parameters<SimModelsReq>,
    ) -> Result<String, String> {
        if let Some(r) = req.repo.as_deref() {
            self.repo_path(r)?;
        }
        Ok(truncate_output(models::inventory(
            &self.root,
            req.repo.as_deref(),
            req.filter.as_deref(),
        )))
    }

    #[tool(
        description = "Inspect ONE JSON model spec without running it (DES-unique): parse it and report its schema, the model/citizen it drives, parameters, runtime knobs, tags, and — for the universal-math DES documents — the math payload (state variables, equations, block graph). Default repo des-engine (its 95 examples/*.json); pass file relative to the repo root, e.g. examples/blackjack-mc.json. Read-only, parse-only."
    )]
    async fn sim_model_inspect(
        &self,
        Parameters(req): Parameters<SimModelInspectReq>,
    ) -> Result<String, String> {
        let repo = req.repo.as_deref().unwrap_or("des-engine");
        let dir = self.repo_path(repo)?;
        let path = crate::spec::resolve_spec(&dir, &req.file)?;
        let meta = std::fs::metadata(&path).map_err(|e| format!("stat failed: {e}"))?;
        if meta.len() > crate::spec::MAX_SPEC_BYTES {
            return Err(format!(
                "{:?} is {} bytes — too large to inspect (cap {})",
                req.file,
                meta.len(),
                crate::spec::MAX_SPEC_BYTES
            ));
        }
        let raw = std::fs::read_to_string(&path).map_err(|e| format!("read failed: {e}"))?;
        let label = format!("{repo}/{}", req.file);
        Ok(truncate_output(crate::spec::inspect(&label, &raw)?))
    }

    #[tool(
        description = "The DES engine core abstractions, drawn from the real code: the FEL scheduler Engine<W> (schedule_at/schedule_after/run_until/now) in discrete-event-system.rs, the model-citizen contract (CitizenRegistry, ModelDescriptor, RunArtifact), and the TS engine's station/tick kernel (DESStation, TimeSteppedStation — deliberately NO global future event list). Offline/embedded."
    )]
    async fn engine_docs(&self) -> Result<String, String> {
        Ok(org_map::ENGINE_DOCS.to_string())
    }

    #[tool(
        description = "Legacy-vs-current engine comparison: the uta-phd → des-engine (TS) → discrete-event-system.rs (Rust) lineage, kernel/clock/solver differences, and why old-outmoded-engine.rs (soccer_engine) is legacy (superseded by akrion-sim). Offline/embedded."
    )]
    async fn engine_comparison(&self) -> Result<String, String> {
        Ok(org_map::ENGINE_COMPARISON.to_string())
    }

    #[tool(
        description = "Org/repo map for discrete-event-systems: which repos exist, where they live TODAY (local ~/codes/ores checkouts migrating into the GitHub org), build/test entry points, and the shared ORESoftware infra (k8s GitOps, dpm migrations, Cloudflare/Squarespace, fiducia secrets, MASH rule). Offline/embedded."
    )]
    async fn org_map(&self) -> Result<String, String> {
        Ok(org_map::ORG_MAP.to_string())
    }

    // ------------------------------------------------------ client telemetry

    #[tool(
        description = "How org clients stream logs/telemetry to Supabase (prefix des_): direct client → Supabase via supabase.rpc('ingest_des_client_log_snapshot'/'ingest_des_client_log_entries') over supabase-js (WASM/TS sim visualizers) or supabase_flutter (Dart), tables des_client_log_snapshots/des_client_log_entries, RLS, realtime, rate limits. Offline/embedded."
    )]
    async fn telemetry_docs(&self) -> Result<String, String> {
        Ok(org_map::TELEMETRY_DOCS.to_string())
    }

    #[tool(
        description = "Recent client-log sessions (des_client_log_snapshots): when, session id, sim id, environment, entry count, trigger, user. Filter by environment/user. Needs SUPABASE_URL + SUPABASE_SERVICE_ROLE_KEY. Read-only."
    )]
    async fn client_log_sessions(
        &self,
        Parameters(req): Parameters<LogSessionsReq>,
    ) -> Result<String, String> {
        let limit = req.limit.unwrap_or(20).clamp(1, 100);
        let mut query = vec![
            (
                "select".to_string(),
                "snapshot_taken_at,session_id,sim_id,environment,log_entries_count,trigger,user_id"
                    .to_string(),
            ),
            ("order".to_string(), "snapshot_taken_at.desc".to_string()),
            ("limit".to_string(), limit.to_string()),
        ];
        if let Some(env) = req.environment.as_deref() {
            safe_pg_filter_value(env, "environment")?;
            query.push(("environment".to_string(), format!("eq.{env}")));
        }
        if let Some(user) = req.user_id.as_deref() {
            safe_pg_filter_value(user, "user_id")?;
            query.push(("user_id".to_string(), format!("eq.{user}")));
        }
        let rows = supabase::rest_get(supabase::SNAPSHOTS_TABLE, &query).await?;
        Ok(truncate_output(format!(
            "recent client-log sessions ({} row(s)):\n{}",
            rows.len(),
            supabase::format_sessions(&rows)
        )))
    }

    #[tool(
        description = "Tail the streamed log entries (des_client_log_entries) for one session, oldest→newest, optional level filter (error/warn/info/debug/trace). Needs SUPABASE_URL + SUPABASE_SERVICE_ROLE_KEY. Read-only."
    )]
    async fn tail_client_logs(
        &self,
        Parameters(req): Parameters<TailLogsReq>,
    ) -> Result<String, String> {
        safe_pg_filter_value(&req.session_id, "session_id")?;
        let limit = req.limit.unwrap_or(100).clamp(1, 500);
        let mut query = vec![
            (
                "select".to_string(),
                "client_timestamp,level,message,stack".to_string(),
            ),
            ("session_id".to_string(), format!("eq.{}", req.session_id)),
            ("order".to_string(), "client_timestamp.desc".to_string()),
            ("limit".to_string(), limit.to_string()),
        ];
        if let Some(level) = req.level.as_deref() {
            if !LOG_LEVELS.contains(&level) {
                return Err(format!("invalid level {level:?} — one of {LOG_LEVELS:?}"));
            }
            query.push(("level".to_string(), format!("eq.{level}")));
        }
        let mut rows = supabase::rest_get(supabase::ENTRIES_TABLE, &query).await?;
        rows.reverse(); // chronological
        Ok(truncate_output(format!(
            "entries for session {} ({} row(s), oldest first):\n{}",
            req.session_id,
            rows.len(),
            supabase::format_entries(&rows)
        )))
    }

    #[tool(
        description = "Recent error/warn counts from des_client_log_entries grouped by message (look-back window in hours, default 24) — the quick 'what is breaking in the sim visualizers' view. Needs SUPABASE_URL + SUPABASE_SERVICE_ROLE_KEY. Read-only."
    )]
    async fn client_error_summary(
        &self,
        Parameters(req): Parameters<ErrorSummaryReq>,
    ) -> Result<String, String> {
        let hours = req.hours.unwrap_or(24).clamp(1, 720);
        let group_by = req.group_by.as_deref().unwrap_or("message");
        if !supabase::GROUP_FIELDS.contains(&group_by) {
            return Err(format!(
                "invalid group_by {group_by:?} — one of {:?}",
                supabase::GROUP_FIELDS
            ));
        }
        let since = (chrono::Utc::now() - chrono::Duration::hours(hours as i64))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        // Always select the field we group by (plus level/message for context).
        let select = format!("level,message,{group_by}");
        let mut query = vec![
            ("select".to_string(), select),
            ("level".to_string(), "in.(error,warn)".to_string()),
            ("created_at".to_string(), format!("gte.{since}")),
            ("limit".to_string(), "1000".to_string()),
        ];
        if let Some(env) = req.environment.as_deref() {
            safe_pg_filter_value(env, "environment")?;
            query.push(("environment".to_string(), format!("eq.{env}")));
        }
        let rows = supabase::rest_get(supabase::ENTRIES_TABLE, &query).await?;
        let grouped = if group_by == "message" {
            supabase::summarize_errors(&rows)
        } else {
            supabase::summarize_field(&rows, group_by)
        };
        Ok(truncate_output(format!(
            "error/warn entries in the last {hours}h grouped by {group_by} ({} sampled, cap 1000):\n{grouped}",
            rows.len(),
        )))
    }

    #[tool(
        description = "Full ordered entry timeline for ONE client session (des_client_log_entries), oldest→newest, with level/url/sim_id/category tags — the 'walk this session end to end' view for root-causing a client error. Needs SUPABASE_URL + SUPABASE_SERVICE_ROLE_KEY. Read-only."
    )]
    async fn client_log_trace(
        &self,
        Parameters(req): Parameters<ClientLogTraceReq>,
    ) -> Result<String, String> {
        safe_pg_filter_value(&req.session_id, "session_id")?;
        let limit = req.limit.unwrap_or(300).clamp(1, 1000);
        let query = vec![
            (
                "select".to_string(),
                "client_timestamp,level,message,url,sim_id,category".to_string(),
            ),
            ("session_id".to_string(), format!("eq.{}", req.session_id)),
            ("order".to_string(), "client_timestamp.asc".to_string()),
            ("limit".to_string(), limit.to_string()),
        ];
        let rows = supabase::rest_get(supabase::ENTRIES_TABLE, &query).await?;
        Ok(truncate_output(format!(
            "trace for session {} ({} entry/entries, oldest first):\n{}",
            req.session_id,
            rows.len(),
            supabase::format_trace(&rows)
        )))
    }

    // --------------------------------------------------------------------- CI

    #[tool(
        description = "Latest GitHub Actions run(s) for every repo in github.com/discrete-event-systems (currently just des-mcp-server.rs; engine repos appear as they migrate in). GITHUB_TOKEN/GH_TOKEN optional for rate limits/private repos. Read-only."
    )]
    async fn ci_status(&self, Parameters(req): Parameters<CiStatusReq>) -> Result<String, String> {
        let out = github::ci_status(req.per_repo.unwrap_or(1)).await?;
        Ok(truncate_output(out))
    }

    // ---------------------------------------------------------------- domains

    #[tool(
        description = "Resolve DNS records for a domain via DNS-over-HTTPS (Cloudflare 1.1.1.1). Registrar-agnostic — works whether DNS is hosted at Cloudflare, Squarespace, or anywhere else. Omit record_type for a sweep of A/AAAA/CNAME/NS/MX/TXT."
    )]
    async fn dns_lookup(
        &self,
        Parameters(req): Parameters<DnsLookupReq>,
    ) -> Result<String, String> {
        let types: Vec<String> = match req.record_type {
            Some(t) => vec![t.to_uppercase()],
            None => ["A", "AAAA", "CNAME", "NS", "MX", "TXT"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        };
        let mut out = format!("DNS for {} (via 1.1.1.1 DoH):\n", req.domain);
        for t in &types {
            let v = domains::doh_query(&req.domain, t).await?;
            out.push_str(&format!("## {t}\n{}\n", domains::format_doh(t, &v)));
        }
        Ok(truncate_output(out))
    }

    #[tool(
        description = "Registration + hosting info for a domain via RDAP: registrar, expiry date with days-remaining, status flags, and nameservers classified by DNS host. Squarespace holds registration for most org domains and has no public DNS API, so this is the canonical way to inspect them."
    )]
    async fn domain_info(
        &self,
        Parameters(req): Parameters<DomainInfoReq>,
    ) -> Result<String, String> {
        let v = domains::rdap_lookup(&req.domain).await?;
        let mut out = domains::summarize_rdap(&v);
        // live NS check often differs from stale registry data — show both
        if let Ok(ns) = domains::doh_query(&req.domain, "NS").await {
            let live = domains::format_doh("NS", &ns);
            out.push_str(&format!("\nlive NS (per 1.1.1.1):\n{live}\n"));
        }
        Ok(truncate_output(out))
    }

    #[tool(
        description = "Check the TLS certificate served at host:port (default 443): subject, issuer, notAfter, and days until expiry."
    )]
    async fn tls_cert_check(
        &self,
        Parameters(req): Parameters<TlsCertReq>,
    ) -> Result<String, String> {
        domains::tls_cert_check(&req.host, req.port.unwrap_or(443)).await
    }

    #[tool(
        description = "List Cloudflare zones visible to the CLOUDFLARE_API_TOKEN env var (read-only; token should be scoped Zone:Read + DNS:Read)."
    )]
    async fn cloudflare_zones(&self) -> Result<String, String> {
        let body = cloudflare::api_get("/zones", &[("per_page", "50")]).await?;
        Ok(truncate_output(format!(
            "Cloudflare zones:\n{}",
            cloudflare::format_zones(&body)
        )))
    }

    #[tool(
        description = "List DNS records in a Cloudflare zone (by zone name), optionally filtered by type and/or exact record name. Requires CLOUDFLARE_API_TOKEN."
    )]
    async fn cloudflare_dns_records(
        &self,
        Parameters(req): Parameters<CfRecordsReq>,
    ) -> Result<String, String> {
        util::safe_hostname(&req.zone)?;
        let zone_id = cloudflare::zone_id_by_name(&req.zone).await?;
        let mut query: Vec<(&str, &str)> = vec![("per_page", "100")];
        let rtype = req.record_type.as_deref().map(str::to_uppercase);
        if let Some(t) = rtype.as_deref() {
            query.push(("type", t));
        }
        if let Some(n) = req.name.as_deref() {
            util::safe_hostname(n)?;
            query.push(("name", n));
        }
        let body = cloudflare::api_get(&format!("/zones/{zone_id}/dns_records"), &query).await?;
        Ok(truncate_output(format!(
            "DNS records in zone {}:\n{}",
            req.zone,
            cloudflare::format_records(&body)
        )))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for DesMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Tools for the discrete-event-systems org (DES engine work). The engine \
                 repos still live LOCALLY under DES_ROOT (default ~/codes/ores) and are \
                 migrating into the GitHub org — org_overview and org_map explain the \
                 layout. Use search_code instead of raw grep (skips target/ and \
                 node_modules/). Builds: cargo_check / engine_tests for the Rust engine \
                 (discrete-event-system.rs; legacy old-outmoded-engine.rs is lib-only), \
                 ts_engine_check for the original TypeScript engine. sim_models lists \
                 runnable models/scenarios (95 JSON specs in des-engine, ~142 demo bins \
                 in the Rust engine). engine_docs/engine_comparison explain the FEL \
                 Engine<W> vs station/tick kernels and the legacy lineage. Client \
                 telemetry (prefix des_): client_log_sessions / tail_client_logs / \
                 client_error_summary read the Supabase tables (SUPABASE_URL + \
                 SUPABASE_SERVICE_ROLE_KEY); telemetry_docs explains the ingest RPCs. \
                 cloudflare_* needs CLOUDFLARE_API_TOKEN; Squarespace-registered domains \
                 are covered via domain_info (RDAP) + dns_lookup (DoH). All tools are \
                 read-only or build-only.",
            )
    }
}
