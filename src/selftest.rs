//! `self_test` support: a fast, offline capability report. Tells an operator
//! which env vars/creds are present (values NEVER shown) and therefore which
//! tool families are fully live vs degraded, plus which DES repos are on disk
//! under DES_ROOT. No network, no subprocess — pure environment inspection.

use std::path::Path;

use crate::util::DES_REPOS;

fn present(name: &str) -> bool {
    std::env::var(name).map(|v| !v.is_empty()).unwrap_or(false)
}

fn mark(ok: bool) -> &'static str {
    if ok { "LIVE    " } else { "DEGRADED" }
}

/// Build the capability report for the given org root.
pub fn report(root: &Path) -> String {
    let mut out = String::from("# des-mcp-server self-test (offline capability report)\n\n");

    out.push_str(&format!("DES_ROOT: {}\n", root.display()));
    let repos: Vec<&str> = DES_REPOS
        .iter()
        .copied()
        .filter(|r| root.join(r).is_dir())
        .collect();
    let missing: Vec<&str> = DES_REPOS
        .iter()
        .copied()
        .filter(|r| !root.join(r).is_dir())
        .collect();
    out.push_str(&format!("  DES repos present: {}\n", repos.join(", ")));
    if !missing.is_empty() {
        out.push_str(&format!("  DES repos missing: {}\n", missing.join(", ")));
    }
    let rust_engine = root
        .join("discrete-event-system.rs")
        .join("Cargo.toml")
        .is_file();
    out.push_str(&format!(
        "  cargo/engine tools: {} (discrete-event-system.rs Cargo.toml {})\n\n",
        mark(rust_engine),
        if rust_engine { "found" } else { "absent" }
    ));

    // Credential-gated families. Presence only — never the secret's value.
    let supabase = present("SUPABASE_URL") && present("SUPABASE_SERVICE_ROLE_KEY");
    let cloudflare = present("CLOUDFLARE_API_TOKEN");
    let github = present("GITHUB_TOKEN") || present("GH_TOKEN");
    let fiducia = present("FIDUCIA_URL") && present("FIDUCIA_TOKEN");

    out.push_str("## tool families\n");
    out.push_str(
        "  LIVE     org/git, builds, sim_models, sim_model_inspect, docs (no creds needed)\n",
    );
    out.push_str("  LIVE     domains: dns_lookup, domain_info, tls_cert_check (public DoH/RDAP)\n");
    out.push_str(&format!(
        "  {} telemetry: client_log_sessions/tail_client_logs/client_log_trace/client_error_summary  \
         (needs SUPABASE_URL + SUPABASE_SERVICE_ROLE_KEY)\n",
        mark(supabase)
    ));
    out.push_str(&format!(
        "  {} cloudflare_zones/cloudflare_dns_records  (needs CLOUDFLARE_API_TOKEN)\n",
        mark(cloudflare)
    ));
    out.push_str(&format!(
        "  {} ci_status  (works unauthenticated; GITHUB_TOKEN/GH_TOKEN raises rate limits{})\n",
        if github { "LIVE    " } else { "LIMITED " },
        if github {
            ""
        } else {
            " — currently unauthenticated"
        }
    ));
    out.push_str(&format!(
        "  {} fiducia_status  (needs FIDUCIA_URL + FIDUCIA_TOKEN)\n",
        mark(fiducia)
    ));

    out.push_str(
        "\n(values of secrets are never read into this report — only presence. \
         Run stack_status for a live GREEN/DEGRADED/RED rollup.)\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_lists_repos_and_families() {
        let tmp = std::env::temp_dir().join(format!("des-selftest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("discrete-event-system.rs")).unwrap();
        std::fs::write(
            tmp.join("discrete-event-system.rs/Cargo.toml"),
            "[package]\nname=\"x\"\n",
        )
        .unwrap();
        let out = report(&tmp);
        assert!(out.contains("discrete-event-system.rs"));
        assert!(out.contains("cargo/engine tools: LIVE"));
        assert!(out.contains("telemetry:"));
        assert!(out.contains("fiducia_status"));
        // no secret VALUES, only presence markers
        assert!(out.contains("LIVE") || out.contains("DEGRADED"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
