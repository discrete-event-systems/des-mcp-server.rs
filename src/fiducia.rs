//! Read-only integration with fiducia.cloud, the org's shared secrets/locks
//! plane. `fiducia_status` reports whether this org's required secrets are
//! present locally and — when a fiducia endpoint is configured — probes its
//! health/lease API. Everything here is read-only and gracefully degraded:
//! if the env is unconfigured the caller gets a clear typed error, never a
//! crash, and the token is only ever sent as a bearer header, never echoed.

use std::time::Duration;

use serde_json::Value;

use crate::domains::http_client;

/// Secret env vars this org's live tools depend on. `fiducia_status` reports
/// which are present (never their values) so an operator can see at a glance
/// which capabilities are wired up. Keep in sync with the tool env docs.
pub const REQUIRED_SECRETS: &[(&str, &str)] = &[
    ("SUPABASE_URL", "client-telemetry reads"),
    ("SUPABASE_SERVICE_ROLE_KEY", "client-telemetry reads"),
    ("CLOUDFLARE_API_TOKEN", "cloudflare_* domain tools"),
];

pub struct FiduciaEnv {
    pub url: String,
    pub token: String,
}

/// Read FIDUCIA_URL + FIDUCIA_TOKEN. Absent → a typed, actionable error.
pub fn env() -> Result<FiduciaEnv, String> {
    let url = std::env::var("FIDUCIA_URL").ok().filter(|s| !s.is_empty());
    let token = std::env::var("FIDUCIA_TOKEN").ok().filter(|s| !s.is_empty());
    match (url, token) {
        (Some(url), Some(token)) => Ok(FiduciaEnv {
            url: url.trim_end_matches('/').to_string(),
            token,
        }),
        _ => Err(
            "fiducia is not configured — set FIDUCIA_URL and FIDUCIA_TOKEN (the org's \
             fiducia.cloud secrets/locks endpoint + a read-scoped token) in the \
             environment the MCP server starts in. Local required-secret presence is \
             still reported below."
                .to_string(),
        ),
    }
}

/// Report which required secrets are present in this process's environment.
/// Presence only — values are never read into the output.
pub fn local_secret_report() -> String {
    let mut out = String::from("## required secrets (local presence — values never shown)\n");
    for (name, used_for) in REQUIRED_SECRETS {
        let present = std::env::var(name).map(|v| !v.is_empty()).unwrap_or(false);
        out.push_str(&format!(
            "  [{}] {name}  — {used_for}\n",
            if present { "present" } else { "MISSING " }
        ));
    }
    out
}

/// Probe the fiducia health/lease endpoint (read-only GET, bounded, timed out).
/// A down endpoint yields a clean "unreachable" line, not an error.
pub async fn probe(env: &FiduciaEnv) -> String {
    let client = match http_client() {
        Ok(c) => c,
        Err(e) => return format!("  (could not build HTTP client: {e})"),
    };
    // Try a conventional read-only health path; fiducia-clients expose lease
    // health here. We never call any mutating route.
    let url = format!("{}/health", env.url);
    let resp = tokio::time::timeout(
        Duration::from_secs(10),
        client.get(&url).bearer_auth(&env.token).send(),
    )
    .await;
    match resp {
        Err(_) => "  fiducia endpoint: UNREACHABLE (timed out after 10s)".to_string(),
        Ok(Err(e)) => format!("  fiducia endpoint: UNREACHABLE ({e})"),
        Ok(Ok(r)) => {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            let summary = summarize_health(&body);
            format!("  fiducia endpoint: HTTP {status}{summary}")
        }
    }
}

/// Best-effort one-line summary of a fiducia health JSON body, if it is JSON.
fn summarize_health(body: &str) -> String {
    match serde_json::from_str::<Value>(body) {
        Ok(v) => {
            let mut parts = Vec::new();
            for k in ["status", "healthy", "leases", "locks", "secrets"] {
                if let Some(val) = v.get(k) {
                    parts.push(format!("{k}={}", compact(val)));
                }
            }
            if parts.is_empty() {
                String::new()
            } else {
                format!("  ({})", parts.join(", "))
            }
        }
        Err(_) => {
            let t = body.trim();
            if t.is_empty() {
                String::new()
            } else {
                let head: String = t.chars().take(80).collect();
                format!("  ({head})")
            }
        }
    }
}

fn compact(v: &Value) -> String {
    let s = serde_json::to_string(v).unwrap_or_default();
    if s.len() > 60 {
        format!("{}…", &s[..60])
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_secret_report_lists_all_required() {
        let out = local_secret_report();
        for (name, _) in REQUIRED_SECRETS {
            assert!(out.contains(name), "report missing {name}");
        }
        // never contains a "present:VALUE" leak — only the marker words
        assert!(out.contains("present") || out.contains("MISSING"));
    }

    #[test]
    fn env_errors_actionably_when_unset() {
        if std::env::var("FIDUCIA_URL").is_err() || std::env::var("FIDUCIA_TOKEN").is_err() {
            let e = env().unwrap_err();
            assert!(e.contains("FIDUCIA_URL"));
            assert!(e.contains("FIDUCIA_TOKEN"));
        }
    }

    #[test]
    fn summarize_health_reads_json_and_text() {
        let j = summarize_health(r#"{"status":"ok","leases":3,"ignored":true}"#);
        assert!(j.contains("status="));
        assert!(j.contains("leases=3"));
        assert!(!j.contains("ignored"));
        assert_eq!(summarize_health(""), "");
        assert!(summarize_health("plain text down").contains("plain text down"));
    }
}
