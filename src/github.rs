//! GitHub Actions CI status for the discrete-event-systems org.
//! Unauthenticated by default; sends a Bearer token when GITHUB_TOKEN or
//! GH_TOKEN is set (higher rate limits, private repos). Read-only.

use serde_json::Value;

use crate::domains::http_client;
use crate::util;

/// GitHub organization for the DES work. Currently only des-mcp-server.rs
/// lives here; the engine repos will migrate in over time (see org_map).
pub const ORG: &str = "discrete-event-systems";

const API_ROOT: &str = "https://api.github.com";

fn token() -> Option<String> {
    std::env::var("GITHUB_TOKEN")
        .or_else(|_| std::env::var("GH_TOKEN"))
        .ok()
        .filter(|t| !t.trim().is_empty())
}

pub async fn get_json(path: &str) -> Result<Value, String> {
    let client = http_client()?;
    let mut req = client
        .get(format!("{API_ROOT}{path}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(t) = token() {
        req = req.bearer_auth(t);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("GET {path} failed: {e}"))?;
    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("GET {path}: response was not JSON: {e}"))?;
    if !status.is_success() {
        return Err(format!(
            "GET {path} returned {status}: {}",
            body.get("message").and_then(Value::as_str).unwrap_or("")
        ));
    }
    Ok(body)
}

/// Names of the org's repos (from GET /orgs/{org}/repos).
pub fn repo_names(body: &Value) -> Vec<String> {
    body.as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r.get("name").and_then(Value::as_str))
        .map(String::from)
        .collect()
}

/// Summarize a `GET /repos/{org}/{repo}/actions/runs?per_page=N` body.
pub fn summarize_runs(repo: &str, body: &Value) -> String {
    let runs = body.get("workflow_runs").and_then(Value::as_array);
    match runs {
        Some(list) if !list.is_empty() => list
            .iter()
            .map(|run| {
                let f = |k: &str| run.get(k).and_then(Value::as_str).unwrap_or("?");
                format!(
                    "  {repo}: {} @ {} — {}{}  ({})",
                    f("name"),
                    f("head_branch"),
                    f("status"),
                    run.get("conclusion")
                        .and_then(Value::as_str)
                        .map(|c| format!("/{c}"))
                        .unwrap_or_default(),
                    f("updated_at"),
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => format!("  {repo}: (no workflow runs)"),
    }
}

/// Latest CI run for every repo in the org.
pub async fn ci_status(per_repo: u32) -> Result<String, String> {
    let repos_body = get_json(&format!("/orgs/{ORG}/repos?per_page=100")).await?;
    let names = repo_names(&repos_body);
    if names.is_empty() {
        return Ok(format!("no repos visible in github.com/{ORG}"));
    }
    let mut out = format!("GitHub Actions status for github.com/{ORG}:\n");
    for name in &names {
        util::safe_segment(name, "repo")?;
        let runs = get_json(&format!(
            "/repos/{ORG}/{name}/actions/runs?per_page={}",
            per_repo.clamp(1, 10)
        ))
        .await;
        match runs {
            Ok(body) => out.push_str(&summarize_runs(name, &body)),
            Err(e) => out.push_str(&format!("  {name}: <{e}>")),
        }
        out.push('\n');
    }
    out.push_str(
        "\n(engine repos still live locally under DES_ROOT and in ORESoftware; \
         they will appear here as they migrate into the org — see org_map)\n",
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn repo_names_extracts_names() {
        let body = json!([{"name": "des-mcp-server.rs"}, {"name": "des-viz.web"}]);
        assert_eq!(repo_names(&body), vec!["des-mcp-server.rs", "des-viz.web"]);
        assert!(repo_names(&json!({})).is_empty());
    }

    #[test]
    fn summarize_runs_renders_run_and_empty() {
        let body = json!({"workflow_runs": [{
            "name": "ci",
            "head_branch": "main",
            "status": "completed",
            "conclusion": "success",
            "updated_at": "2026-07-17T00:00:00Z"
        }]});
        let s = summarize_runs("des-mcp-server.rs", &body);
        assert!(s.contains("des-mcp-server.rs: ci @ main — completed/success"));
        assert!(summarize_runs("x", &json!({})).contains("no workflow runs"));
    }
}
