//! End-to-end tests: spawn the real binary, speak MCP JSON-RPC over stdio
//! against a hermetic temp "org root" (scratch git repos named after the
//! DES repo allowlist), and check tool behavior including error paths.
//! No network access required.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

struct McpProc {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl McpProc {
    fn spawn(org_root: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_des-mcp-server"))
            .env("DES_ROOT", org_root)
            // hermetic: never inherit real credentials into the tests
            .env_remove("SUPABASE_URL")
            .env_remove("SUPABASE_SERVICE_ROLE_KEY")
            .env_remove("CLOUDFLARE_API_TOKEN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn des-mcp-server");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut proc = McpProc {
            child,
            stdin,
            stdout,
            next_id: 1,
        };
        let init = proc.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "integration-test", "version": "0"}
            }),
        );
        assert_eq!(
            init.pointer("/result/serverInfo/name")
                .and_then(Value::as_str),
            Some("des-mcp-server")
        );
        assert!(
            init.pointer("/result/instructions")
                .and_then(Value::as_str)
                .unwrap_or("")
                .contains("org_overview"),
            "instructions should mention org_overview"
        );
        proc.notify("notifications/initialized");
        proc
    }

    fn send(&mut self, msg: &Value) {
        let line = serde_json::to_string(msg).unwrap();
        writeln!(self.stdin, "{line}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn notify(&mut self, method: &str) {
        self.send(&json!({"jsonrpc": "2.0", "method": method}));
    }

    /// Send a request and block until its response line arrives.
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let mut line = String::new();
            let n = self.stdout.read_line(&mut line).expect("read response");
            assert!(n > 0, "server closed stdout before responding to {method}");
            let v: Value = serde_json::from_str(&line).expect("response is JSON");
            if v.get("id").and_then(Value::as_u64) == Some(id) {
                return v;
            }
            // ignore unrelated notifications from the server
        }
    }

    fn call_tool(&mut self, name: &str, args: Value) -> (bool, String) {
        let resp = self.request("tools/call", json!({"name": name, "arguments": args}));
        let result = resp.get("result").unwrap_or_else(|| {
            panic!("tools/call {name} returned no result: {resp}");
        });
        let is_error = result
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let text = result
            .pointer("/content/0/text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        (is_error, text)
    }
}

impl Drop for McpProc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn git_in(repo: &Path, args: &[&str]) {
    let st = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "-c",
            "user.email=it@test",
            "-c",
            "user.name=Integration Test",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("run git");
    assert!(st.success(), "git {args:?} failed");
}

/// Build a temp org root containing tiny committed git repos named after
/// the DES allowlist (plus a non-DES repo that must stay invisible).
fn temp_org(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("des-mcp-it-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);

    // fake current Rust engine
    let rust = root.join("discrete-event-system.rs");
    std::fs::create_dir_all(rust.join("src/bin")).unwrap();
    std::fs::create_dir_all(rust.join("examples")).unwrap();
    std::fs::write(
        rust.join("src/lib.rs"),
        "// SENTINEL_PATTERN_XYZ lives here\npub fn f() {}\n",
    )
    .unwrap();
    std::fs::write(rust.join("src/bin/main_elevator.rs"), "fn main() {}\n").unwrap();
    std::fs::write(rust.join("examples/fel_compare_mm1.rs"), "fn main() {}\n").unwrap();
    std::fs::write(
        rust.join("Cargo.toml"),
        "[package]\nname = \"des_engine\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    git_in(&rust, &["init", "-q", "-b", "main"]);
    git_in(&rust, &["add", "-A"]);
    git_in(&rust, &["commit", "-q", "-m", "initial engine commit"]);

    // fake original TS engine with JSON model specs
    let ts = root.join("des-engine");
    std::fs::create_dir_all(ts.join("examples")).unwrap();
    std::fs::write(ts.join("examples/tiger-pomdp.json"), "{}\n").unwrap();
    std::fs::write(ts.join("examples/blackjack-mc.json"), "{}\n").unwrap();
    std::fs::write(ts.join("package.json"), "{\"name\": \"uta-phd-des\"}\n").unwrap();
    git_in(&ts, &["init", "-q", "-b", "main"]);
    git_in(&ts, &["add", "-A"]);
    git_in(&ts, &["commit", "-q", "-m", "initial ts commit"]);

    // a non-DES repo in the same root: must never be addressable
    let other = root.join("k8s-cluster");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("readme.md"), "not a DES repo\n").unwrap();
    git_in(&other, &["init", "-q", "-b", "main"]);
    git_in(&other, &["add", "-A"]);
    git_in(&other, &["commit", "-q", "-m", "unrelated"]);

    root
}

#[test]
fn tools_list_contains_all_tool_families() {
    let org = temp_org("list");
    let mut mcp = McpProc::spawn(&org);
    let resp = mcp.request("tools/list", json!({}));
    let names: Vec<&str> = resp
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .expect("tools array")
        .iter()
        .filter_map(|t| t.get("name").and_then(Value::as_str))
        .collect();
    for expected in [
        // org/git
        "org_overview",
        "repo_status",
        "recent_commits",
        "search_code",
        // builds
        "cargo_check",
        "engine_tests",
        "ts_engine_check",
        // models & docs
        "sim_models",
        "engine_docs",
        "engine_comparison",
        "org_map",
        // telemetry
        "telemetry_docs",
        "client_log_sessions",
        "tail_client_logs",
        "client_error_summary",
        // CI
        "ci_status",
        // domains
        "dns_lookup",
        "domain_info",
        "tls_cert_check",
        "cloudflare_zones",
        "cloudflare_dns_records",
    ] {
        assert!(
            names.contains(&expected),
            "missing tool {expected}: {names:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&org);
}

#[test]
fn org_and_git_tools_work_against_temp_org() {
    let org = temp_org("git");
    let mut mcp = McpProc::spawn(&org);

    let (err, text) = mcp.call_tool("org_overview", json!({}));
    assert!(!err, "org_overview errored: {text}");
    assert!(text.contains("discrete-event-system.rs"));
    assert!(text.contains("des-engine"));
    assert!(text.contains("branch: main"));
    assert!(text.contains("initial engine commit"));
    assert!(
        !text.contains("k8s-cluster"),
        "non-DES repos must not appear: {text}"
    );

    let (err, text) = mcp.call_tool("repo_status", json!({"repo": "discrete-event-system.rs"}));
    assert!(!err, "repo_status errored: {text}");
    assert!(text.contains("## branch"));
    assert!(text.contains("main"));

    let (err, text) = mcp.call_tool(
        "search_code",
        json!({"pattern": "SENTINEL_PATTERN_[A-Z]+", "max_matches": 10}),
    );
    assert!(!err, "search_code errored: {text}");
    assert!(
        text.contains("discrete-event-system.rs/src/lib.rs:1:"),
        "got: {text}"
    );

    let (err, text) = mcp.call_tool(
        "search_code",
        json!({"pattern": "DEFINITELY_NOT_PRESENT_ANYWHERE"}),
    );
    assert!(!err);
    assert!(text.contains("no matches"));

    let (err, text) = mcp.call_tool("recent_commits", json!({"repo": "des-engine", "count": 5}));
    assert!(!err, "recent_commits errored: {text}");
    assert!(text.contains("initial ts commit"));

    let _ = std::fs::remove_dir_all(&org);
}

#[test]
fn sim_models_inventory_lists_specs_and_bins() {
    let org = temp_org("models");
    let mut mcp = McpProc::spawn(&org);

    let (err, text) = mcp.call_tool("sim_models", json!({}));
    assert!(!err, "sim_models errored: {text}");
    assert!(text.contains("examples/tiger-pomdp.json"));
    assert!(text.contains("src/bin/main_elevator.rs"));
    assert!(text.contains("examples/fel_compare_mm1.rs"));
    assert!(text.contains("npm run from-json"));

    let (err, text) = mcp.call_tool("sim_models", json!({"filter": "tiger"}));
    assert!(!err);
    assert!(text.contains("tiger-pomdp.json"));
    assert!(!text.contains("blackjack"));

    let (err, text) = mcp.call_tool("sim_models", json!({"repo": "des-engine"}));
    assert!(!err);
    assert!(!text.contains("main_elevator.rs"));

    let _ = std::fs::remove_dir_all(&org);
}

#[test]
fn embedded_docs_tools_serve_org_specific_content() {
    let org = temp_org("docs");
    let mut mcp = McpProc::spawn(&org);

    let (err, text) = mcp.call_tool("org_map", json!({}));
    assert!(!err);
    assert!(text.contains("discrete-event-systems"));
    assert!(text.contains("DES_ROOT"));
    assert!(text.contains("migrate"));
    assert!(text.contains("dpm"));

    let (err, text) = mcp.call_tool("engine_docs", json!({}));
    assert!(!err);
    assert!(text.contains("Engine<W>"));
    assert!(text.contains("schedule_at"));
    assert!(text.contains("DESStation"));

    let (err, text) = mcp.call_tool("engine_comparison", json!({}));
    assert!(!err);
    assert!(text.contains("soccer_engine"));
    assert!(text.contains("akrion-sim"));

    let (err, text) = mcp.call_tool("telemetry_docs", json!({}));
    assert!(!err);
    assert!(text.contains("ingest_des_client_log_snapshot"));
    assert!(text.contains("des_client_log_entries"));
    assert!(text.contains("supabase-js"));

    let _ = std::fs::remove_dir_all(&org);
}

#[test]
fn invalid_inputs_surface_as_tool_errors_not_crashes() {
    let org = temp_org("err");
    let mut mcp = McpProc::spawn(&org);

    // path traversal in repo name
    let (err, text) = mcp.call_tool("repo_status", json!({"repo": "../../etc"}));
    assert!(err, "traversal should be a tool error");
    assert!(text.contains("invalid repo name"));

    // a real repo in the root that is NOT on the DES allowlist
    let (err, text) = mcp.call_tool("repo_status", json!({"repo": "k8s-cluster"}));
    assert!(err);
    assert!(text.contains("not a DES repo"));

    // allowlisted but absent on disk
    let (err, text) = mcp.call_tool("repo_status", json!({"repo": "old-outmoded-engine.rs"}));
    assert!(err);
    assert!(text.contains("no such repo dir"));

    // flag injection into git log branch
    let (err, text) = mcp.call_tool(
        "recent_commits",
        json!({"repo": "des-engine", "branch": "--output=/tmp/pwned"}),
    );
    assert!(err);
    assert!(text.contains("invalid branch"));

    // bad hostname for TLS check
    let (err, text) = mcp.call_tool("tls_cert_check", json!({"host": "bad host;id"}));
    assert!(err);
    assert!(text.contains("invalid hostname"));

    // bad domain for DoH lookup (rejected before any network I/O)
    let (err, text) = mcp.call_tool("dns_lookup", json!({"domain": "exa mple.com"}));
    assert!(err);
    assert!(text.contains("invalid hostname"));

    // PostgREST filter injection in session id
    let (err, text) = mcp.call_tool(
        "tail_client_logs",
        json!({"session_id": "x)&select=*&limit=9999"}),
    );
    assert!(err);
    assert!(text.contains("invalid session_id"));

    // bogus log level
    let (err, text) = mcp.call_tool(
        "tail_client_logs",
        json!({"session_id": "sess-1", "level": "loud"}),
    );
    assert!(err);
    assert!(text.contains("invalid level"));

    // server must still be alive and responsive after all those errors
    let (err, text) = mcp.call_tool("org_overview", json!({}));
    assert!(!err, "server wedged after error paths: {text}");

    let _ = std::fs::remove_dir_all(&org);
}

#[test]
fn telemetry_and_cloudflare_error_actionably_without_env() {
    let org = temp_org("env");
    let mut mcp = McpProc::spawn(&org);

    // env vars are stripped in spawn(), so these must fail with guidance
    let (err, text) = mcp.call_tool("client_log_sessions", json!({}));
    assert!(err);
    assert!(text.contains("SUPABASE_URL"), "got: {text}");
    assert!(text.contains("SUPABASE_SERVICE_ROLE_KEY"));

    let (err, text) = mcp.call_tool("client_error_summary", json!({"hours": 4}));
    assert!(err);
    assert!(text.contains("SUPABASE_URL"));

    let (err, text) = mcp.call_tool("cloudflare_zones", json!({}));
    assert!(err);
    assert!(text.contains("CLOUDFLARE_API_TOKEN"), "got: {text}");

    let _ = std::fs::remove_dir_all(&org);
}

#[test]
fn cargo_check_runs_in_the_fake_engine_repo() {
    let org = temp_org("check");
    let mut mcp = McpProc::spawn(&org);

    // default repo is discrete-event-system.rs; the fake crate is valid
    let (err, text) = mcp.call_tool("cargo_check", json!({}));
    assert!(!err, "cargo_check errored: {text}");
    assert!(text.contains("cargo check in discrete-event-system.rs: OK"));

    // cargo tools must refuse non-Rust repos
    let (err, text) = mcp.call_tool("cargo_check", json!({"repo": "des-engine"}));
    assert!(err);
    assert!(text.contains("no Cargo.toml"));

    let _ = std::fs::remove_dir_all(&org);
}
