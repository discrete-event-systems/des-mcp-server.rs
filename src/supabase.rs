//! Read-only debugging tools over the org's client-telemetry tables
//! (des_client_log_snapshots / des_client_log_entries) via Supabase
//! PostgREST. Auth: SUPABASE_URL + SUPABASE_SERVICE_ROLE_KEY (service-role
//! reads are allowed by the des_service_all_* RLS policies; clients only
//! ever hold the anon key, which is INSERT-only).

use std::collections::BTreeMap;

use serde_json::Value;

use crate::domains::http_client;

pub const SNAPSHOTS_TABLE: &str = "des_client_log_snapshots";
pub const ENTRIES_TABLE: &str = "des_client_log_entries";

#[derive(Debug)]
pub struct SupabaseEnv {
    pub url: String,
    pub key: String,
}

pub fn env() -> Result<SupabaseEnv, String> {
    let url = std::env::var("SUPABASE_URL").ok().filter(|s| !s.is_empty());
    let key = std::env::var("SUPABASE_SERVICE_ROLE_KEY")
        .ok()
        .filter(|s| !s.is_empty());
    match (url, key) {
        (Some(url), Some(key)) => Ok(SupabaseEnv {
            url: url.trim_end_matches('/').to_string(),
            key,
        }),
        _ => Err(
            "SUPABASE_URL and/or SUPABASE_SERVICE_ROLE_KEY are not set. Export both \
             (Supabase project → Settings → API; use the service_role key — reads on the \
             des_client_log_* tables are RLS-restricted to service_role) in the \
             environment the MCP server starts in."
                .to_string(),
        ),
    }
}

/// GET a PostgREST path with query params; returns the JSON array of rows.
pub async fn rest_get(path: &str, query: &[(String, String)]) -> Result<Vec<Value>, String> {
    let env = env()?;
    let client = http_client()?;
    let resp = client
        .get(format!("{}/rest/v1/{path}", env.url))
        .header("apikey", &env.key)
        .bearer_auth(&env.key)
        .query(query)
        .send()
        .await
        .map_err(|e| format!("supabase request failed: {e}"))?;
    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("supabase response was not JSON: {e}"))?;
    if !status.is_success() {
        return Err(format!(
            "supabase PostgREST error (HTTP {status}): {}. If the tables are missing, \
             apply supabase/schema.sql (dpm-style declarative) to the project first.",
            serde_json::to_string(&body).unwrap_or_default()
        ));
    }
    body.as_array()
        .cloned()
        .ok_or_else(|| "expected a JSON array of rows from PostgREST".to_string())
}

fn s<'a>(row: &'a Value, key: &str) -> &'a str {
    row.get(key).and_then(Value::as_str).unwrap_or("-")
}

/// Recent snapshot rows → session listing.
pub fn format_sessions(rows: &[Value]) -> String {
    if rows.is_empty() {
        return "(no snapshots match)".to_string();
    }
    rows.iter()
        .map(|r| {
            format!(
                "  {}  session={}  sim={}  env={}  entries={}  trigger={}  user={}",
                s(r, "snapshot_taken_at"),
                s(r, "session_id"),
                s(r, "sim_id"),
                s(r, "environment"),
                r.get("log_entries_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                r.pointer("/trigger/type")
                    .and_then(Value::as_str)
                    .unwrap_or("-"),
                s(r, "user_id"),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Entry rows (chronological) → tail listing.
pub fn format_entries(rows: &[Value]) -> String {
    if rows.is_empty() {
        return "(no entries match)".to_string();
    }
    rows.iter()
        .map(|r| {
            let mut line = format!(
                "  {}  [{}] {}",
                s(r, "client_timestamp"),
                s(r, "level"),
                s(r, "message"),
            );
            if let Some(stack) = r.get("stack").and_then(Value::as_str)
                && !stack.is_empty()
            {
                let first = stack.lines().next().unwrap_or("");
                line.push_str(&format!("\n      stack: {first}"));
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Group error/warn rows by (level, message) with counts, most frequent first.
pub fn summarize_errors(rows: &[Value]) -> String {
    if rows.is_empty() {
        return "(no error/warn entries in the window)".to_string();
    }
    let mut counts: BTreeMap<(String, String), u64> = BTreeMap::new();
    for r in rows {
        let level = s(r, "level").to_string();
        let mut msg = s(r, "message").to_string();
        if msg.len() > 160 {
            let mut cut = 160;
            while !msg.is_char_boundary(cut) {
                cut -= 1;
            }
            msg.truncate(cut);
            msg.push('…');
        }
        *counts.entry((level, msg)).or_insert(0) += 1;
    }
    let mut items: Vec<_> = counts.into_iter().collect();
    items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    items
        .into_iter()
        .map(|((level, msg), n)| format!("  {n:5}×  [{level}] {msg}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Fields the triage tools may group error/warn entries by (beyond message).
/// Restricting to this allowlist keeps the `select=` and grouping stable and
/// prevents an arbitrary column name from reaching PostgREST.
pub const GROUP_FIELDS: &[&str] = &["message", "url", "sim_id", "user_id", "session_id", "level"];

/// Group rows by a single field's value, counts descending. Used by
/// client_error_summary's `group_by` to localize failures to a route/model/
/// session/user rather than only a message.
pub fn summarize_field(rows: &[Value], field: &str) -> String {
    if rows.is_empty() {
        return "(no error/warn entries in the window)".to_string();
    }
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for r in rows {
        let mut key = s(r, field).to_string();
        if key.len() > 160 {
            let mut cut = 160;
            while !key.is_char_boundary(cut) {
                cut -= 1;
            }
            key.truncate(cut);
            key.push('…');
        }
        *counts.entry(key).or_insert(0) += 1;
    }
    let mut items: Vec<_> = counts.into_iter().collect();
    items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    items
        .into_iter()
        .take(100)
        .map(|(key, n)| format!("  {n:5}×  {key}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// One session's full ordered timeline (client_log_trace): richer than a tail
/// — includes level, url, sim_id, and category so a session can be walked
/// end-to-end. Rows must already be chronological (oldest first).
pub fn format_trace(rows: &[Value]) -> String {
    if rows.is_empty() {
        return "(no entries for this session)".to_string();
    }
    rows.iter()
        .map(|r| {
            let mut line = format!(
                "  {}  [{}] {}",
                s(r, "client_timestamp"),
                s(r, "level"),
                s(r, "message"),
            );
            let sim = r.get("sim_id").and_then(Value::as_str).unwrap_or("");
            let url = r.get("url").and_then(Value::as_str).unwrap_or("");
            let cat = r.get("category").and_then(Value::as_str).unwrap_or("");
            let mut tags = Vec::new();
            if !sim.is_empty() {
                tags.push(format!("sim={sim}"));
            }
            if !cat.is_empty() {
                tags.push(format!("cat={cat}"));
            }
            if !url.is_empty() {
                tags.push(format!("url={url}"));
            }
            if !tags.is_empty() {
                line.push_str(&format!("\n      {}", tags.join("  ")));
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summarize_field_groups_by_url_and_sorts() {
        let mk = |url: &str| json!({"url": url, "level": "error", "message": "x"});
        let rows = vec![mk("/sim/a"), mk("/sim/a"), mk("/sim/b")];
        let out = summarize_field(&rows, "url");
        let first = out.lines().next().unwrap();
        assert!(first.contains("2×"));
        assert!(first.contains("/sim/a"));
        assert_eq!(summarize_field(&[], "url"), "(no error/warn entries in the window)");
    }

    #[test]
    fn format_trace_includes_sim_and_url_tags() {
        let rows = vec![json!({
            "client_timestamp": "2026-07-17T01:02:03Z",
            "level": "error", "message": "boom",
            "sim_id": "tiger-pomdp", "url": "/viz", "category": "render"
        })];
        let out = format_trace(&rows);
        assert!(out.contains("[error] boom"));
        assert!(out.contains("sim=tiger-pomdp"));
        assert!(out.contains("url=/viz"));
        assert!(out.contains("cat=render"));
        assert_eq!(format_trace(&[]), "(no entries for this session)");
    }

    #[test]
    fn env_error_is_actionable_when_unset() {
        if std::env::var("SUPABASE_URL").is_err()
            || std::env::var("SUPABASE_SERVICE_ROLE_KEY").is_err()
        {
            let err = env().unwrap_err();
            assert!(err.contains("SUPABASE_URL"));
            assert!(err.contains("SUPABASE_SERVICE_ROLE_KEY"));
        }
    }

    #[test]
    fn format_sessions_renders_fields() {
        let rows = vec![json!({
            "snapshot_taken_at": "2026-07-17T01:02:03Z",
            "session_id": "sess-1",
            "sim_id": "tiger-pomdp",
            "environment": "production",
            "log_entries_count": 42,
            "trigger": {"type": "error"},
            "user_id": "0b6e6d3c-2f6e-4c39-9e6e-8f0a1b2c3d4e"
        })];
        let out = format_sessions(&rows);
        assert!(out.contains("sess-1"));
        assert!(out.contains("sim=tiger-pomdp"));
        assert!(out.contains("entries=42"));
        assert!(out.contains("trigger=error"));
        assert_eq!(format_sessions(&[]), "(no snapshots match)");
    }

    #[test]
    fn format_entries_includes_first_stack_line() {
        let rows = vec![json!({
            "client_timestamp": "2026-07-17T01:02:03Z",
            "level": "error",
            "message": "boom",
            "stack": "Error: boom\n  at sim.tick (viz.js:10)"
        })];
        let out = format_entries(&rows);
        assert!(out.contains("[error] boom"));
        assert!(out.contains("stack: Error: boom"));
        assert!(!out.contains("viz.js"), "only the first stack line");
        assert_eq!(format_entries(&[]), "(no entries match)");
    }

    #[test]
    fn summarize_errors_groups_and_sorts() {
        let mk = |level: &str, msg: &str| json!({"level": level, "message": msg});
        let rows = vec![
            mk("error", "queue underflow"),
            mk("error", "queue underflow"),
            mk("warn", "slow frame"),
        ];
        let out = summarize_errors(&rows);
        let first = out.lines().next().unwrap();
        assert!(first.contains("2×"));
        assert!(first.contains("queue underflow"));
        assert!(out.contains("[warn] slow frame"));
        assert_eq!(
            summarize_errors(&[]),
            "(no error/warn entries in the window)"
        );
    }

    #[test]
    fn summarize_errors_truncates_long_messages() {
        let long = "x".repeat(500);
        let rows = vec![json!({"level": "error", "message": long})];
        let out = summarize_errors(&rows);
        assert!(out.contains('…'));
        assert!(out.len() < 300);
    }
}
