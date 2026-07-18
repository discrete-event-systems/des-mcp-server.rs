//! MCP resources and prompts for the discrete-event-systems org.
//!
//! Resources expose the embedded org map, engine/telemetry docs, and the
//! declarative Supabase schema so clients can load them WITHOUT a tool call
//! (orgmap://, docs://, schema://). Prompts are canned, parameterized
//! workflows (engine_readiness, triage_client_errors, domain_audit) that
//! steer a client through this server's tools. Everything here is static /
//! embedded — no I/O.

use rmcp::model::{
    GetPromptResult, Prompt, PromptArgument, PromptMessage, ReadResourceResult, Resource,
    ResourceContents, Role,
};

use crate::org_map;

/// The declarative telemetry schema, embedded so it is readable as a resource.
const SCHEMA_SQL: &str = include_str!("../supabase/schema.sql");

/// (uri, human name, description, mime, body) for each readable resource.
fn entries() -> Vec<(
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    String,
)> {
    vec![
        (
            "orgmap://discrete-event-systems",
            "org map",
            "Repo map, migration plan, build entry points, and shared ORESoftware infra.",
            "text/markdown",
            org_map::ORG_MAP.to_string(),
        ),
        (
            "docs://engine",
            "engine core docs",
            "DES engine abstractions from the real code (FEL Engine<W>, model citizens, TS station/tick kernel).",
            "text/markdown",
            org_map::ENGINE_DOCS.to_string(),
        ),
        (
            "docs://engine-comparison",
            "legacy-vs-current engines",
            "The uta-phd → des-engine (TS) → discrete-event-system.rs (Rust) lineage and why soccer_engine is legacy.",
            "text/markdown",
            org_map::ENGINE_COMPARISON.to_string(),
        ),
        (
            "docs://telemetry",
            "client telemetry pattern",
            "How org clients stream logs to Supabase (ingest RPCs, tables, RLS, realtime).",
            "text/markdown",
            org_map::TELEMETRY_DOCS.to_string(),
        ),
        (
            "schema://telemetry",
            "supabase/schema.sql",
            "Declarative (dpm-style) schema: des_client_log_* tables, ingest RPCs, RLS, realtime publication.",
            "application/sql",
            SCHEMA_SQL.to_string(),
        ),
    ]
}

/// All resources this server advertises.
pub fn resources() -> Vec<Resource> {
    entries()
        .into_iter()
        .map(|(uri, name, desc, mime, _body)| {
            Resource::new(uri, name)
                .with_description(desc)
                .with_mime_type(mime)
        })
        .collect()
}

/// Read one resource by URI. `None` if the URI is unknown.
pub fn read(uri: &str) -> Option<ReadResourceResult> {
    entries()
        .into_iter()
        .find(|(u, ..)| *u == uri)
        .map(|(u, _name, _desc, _mime, body)| {
            ReadResourceResult::new(vec![ResourceContents::text(body, u)])
        })
}

// -------------------------------------------------------------------- prompts

/// All prompts this server advertises.
pub fn prompts() -> Vec<Prompt> {
    vec![
        Prompt::new(
            "engine_readiness",
            Some("Assess whether the DES engine is in a shippable state (build + tests + CI)."),
            None,
        ),
        Prompt::new(
            "triage_client_errors",
            Some("Triage what is breaking in the sim visualizers from Supabase client telemetry."),
            Some(vec![
                PromptArgument::new("hours")
                    .with_description("look-back window in hours (default 24)")
                    .with_required(false),
                PromptArgument::new("environment")
                    .with_description("production / staging / development (optional)")
                    .with_required(false),
            ]),
        ),
        Prompt::new(
            "domain_audit",
            Some("Audit a domain's registration, DNS host, and TLS certificate."),
            Some(vec![
                PromptArgument::new("domain")
                    .with_description("registrable domain to audit, e.g. example.dev")
                    .with_required(true),
            ]),
        ),
    ]
}

fn arg<'a>(
    args: &'a Option<serde_json::Map<String, serde_json::Value>>,
    key: &str,
) -> Option<&'a str> {
    args.as_ref()?.get(key).and_then(|v| v.as_str())
}

/// Render a prompt by name, weaving in any provided arguments. Returns `None`
/// for unknown prompt names (the caller maps that to a protocol error).
pub fn get_prompt(
    name: &str,
    args: &Option<serde_json::Map<String, serde_json::Value>>,
) -> Option<GetPromptResult> {
    let body = match name {
        "engine_readiness" => "Assess the discrete-event-systems engine's readiness. Steps:\n\
             1. Call stack_status for the GREEN/DEGRADED/RED rollup.\n\
             2. Call cargo_check {repo: \"discrete-event-system.rs\"} and report any errors.\n\
             3. Call engine_tests (all_targets=true is the CI matrix); note that ~164 legacy \
             failures are PRE-EXISTING — compare the failing set against a baseline rather than \
             raw counts.\n\
             4. Call ci_status for the latest GitHub Actions result.\n\
             Summarize as ready / not-ready with the specific failing checks named."
            .to_string(),
        "triage_client_errors" => {
            let hours = arg(args, "hours").unwrap_or("24");
            let env = arg(args, "environment");
            let env_clause = env
                .map(|e| format!(" in the {e} environment"))
                .unwrap_or_default();
            let env_json = env
                .map(|e| format!(", environment: \"{e}\""))
                .unwrap_or_default();
            format!(
                "Triage what is breaking in the sim visualizers{env_clause} over the last {hours}h.\n\
                 1. Call client_error_summary {{hours: {hours}{env_json}}} for the top error/warn \
                 messages by frequency.\n\
                 2. For the worst offenders, call client_error_summary with group_by \"url\" and \
                 \"sim_id\" to localize them to a route/model.\n\
                 3. Call client_log_sessions to find affected sessions, then client_log_trace on \
                 one to see the full ordered timeline leading to the error.\n\
                 Report the top failures, where they occur, and a likely root cause."
            )
        }
        "domain_audit" => {
            let domain = arg(args, "domain").unwrap_or("<domain>");
            format!(
                "Audit the domain {domain}.\n\
                 1. Call domain_info {{domain: \"{domain}\"}} for registrar, expiry (days left), \
                 and the nameserver DNS-host classification (Cloudflare vs Squarespace).\n\
                 2. Call dns_lookup {{domain: \"{domain}\"}} for the live A/AAAA/CNAME/NS/MX/TXT \
                 sweep.\n\
                 3. Call tls_cert_check {{host: \"{domain}\"}} for certificate issuer and \
                 days-until-expiry.\n\
                 Flag anything expiring within 30 days and any mismatch between the registrar's \
                 nameservers and the live NS."
            )
        }
        _ => return None,
    };
    Some(
        GetPromptResult::new(vec![PromptMessage::new_text(Role::User, body)])
            .with_description(format!("Canned workflow: {name}")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resources_are_readable_and_org_specific() {
        let rs = resources();
        assert_eq!(rs.len(), 5);
        let uris: Vec<&str> = rs.iter().map(|r| r.uri.as_str()).collect();
        assert!(uris.contains(&"orgmap://discrete-event-systems"));
        assert!(uris.contains(&"schema://telemetry"));
        // every advertised resource must actually read back
        for r in &rs {
            let got = read(&r.uri).expect("advertised resource must read");
            assert_eq!(got.contents.len(), 1);
        }
        assert!(read("does://not-exist").is_none());
    }

    #[test]
    fn schema_resource_carries_the_real_sql() {
        let got = read("schema://telemetry").unwrap();
        if let ResourceContents::TextResourceContents { text, .. } = &got.contents[0] {
            assert!(text.contains("des_client_log_snapshots"));
            assert!(text.contains("ingest_des_client_log_entries"));
        } else {
            panic!("expected text contents");
        }
    }

    #[test]
    fn prompts_list_and_render_with_args() {
        let ps = prompts();
        assert_eq!(ps.len(), 3);
        let names: Vec<&str> = ps.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"engine_readiness"));

        let r = get_prompt("engine_readiness", &None).unwrap();
        let text = match &r.messages[0].content {
            rmcp::model::ContentBlock::Text(t) => t.text.clone(),
            _ => panic!("expected text"),
        };
        assert!(text.contains("stack_status"));
        assert!(text.contains("PRE-EXISTING"));

        let mut args = serde_json::Map::new();
        args.insert("hours".into(), serde_json::json!("6"));
        args.insert("environment".into(), serde_json::json!("production"));
        let r = get_prompt("triage_client_errors", &Some(args)).unwrap();
        let text = match &r.messages[0].content {
            rmcp::model::ContentBlock::Text(t) => t.text.clone(),
            _ => panic!("expected text"),
        };
        assert!(text.contains("last 6h"));
        assert!(text.contains("production"));

        assert!(get_prompt("nope", &None).is_none());
    }

    #[test]
    fn domain_audit_prompt_requires_domain_and_interpolates() {
        let mut args = serde_json::Map::new();
        args.insert("domain".into(), serde_json::json!("example.dev"));
        let r = get_prompt("domain_audit", &Some(args)).unwrap();
        let text = match &r.messages[0].content {
            rmcp::model::ContentBlock::Text(t) => t.text.clone(),
            _ => panic!("expected text"),
        };
        assert!(text.contains("example.dev"));
        assert!(text.contains("tls_cert_check"));
    }
}
