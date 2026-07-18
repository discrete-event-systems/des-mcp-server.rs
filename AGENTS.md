# Agent guidelines — des-mcp-server.rs

MCP server exposing read-only/build-only tools for the discrete-event-systems
org over stdio. See README.md for the tool table and env configuration.

## Hard rules

- **stdout is the MCP wire.** Never print or log to stdout in the binary
  path; anything human-facing goes to stderr (`eprintln!` in `main.rs`).
- **Tools stay read-only or build-only.** No deletes, no moves, no git
  history rewrites, no write-capable cloud tools. The Supabase telemetry
  tools are SELECT-only over PostgREST; the Cloudflare token should be
  scoped Zone:Read + DNS:Read. Any write tool needs explicit operator
  sign-off first.
- **Only DES repos are addressable.** The org root (`DES_ROOT`, default
  `~/codes/ores`) holds many unrelated repos; every repo-taking tool
  validates against the allowlist in `util::DES_REPOS`. When repos migrate
  into github.com/discrete-event-systems, update that list AND the embedded
  docs in `src/org_map.rs`.
- **Validate everything that reaches a CLI or URL.** `safe_segment` /
  `safe_token` / `safe_hostname` / `safe_pg_filter_value` in `src/util.rs`;
  never build shell strings from user input (the only `bash -c` is the
  openssl pipeline over a validated hostname).
- Never log secret values (`SUPABASE_SERVICE_ROLE_KEY`,
  `CLOUDFLARE_API_TOKEN`, `GITHUB_TOKEN`) — they are only ever attached as
  headers and must not appear in error strings or tool results.

## Where things live

- `src/server.rs` — the `#[tool_router]` impl (26 tools) + the `ServerHandler`
  resource/prompt methods (`list_resources`/`read_resource`/`list_prompts`/
  `get_prompt`); thin wrappers so logic stays unit-testable. `src/util.rs` —
  subprocess/validation helpers + the DES repo allowlist. `src/models.rs` —
  sim-model inventory rules. `src/spec.rs` — the DES-unique JSON model-spec
  inspector (`sim_model_inspect`; path-safe, parse-only). `src/catalog.rs` —
  the MCP **resources** (orgmap://, docs://, schema://) and **prompts**
  (engine_readiness, triage_client_errors, domain_audit); embeds
  `supabase/schema.sql` via `include_str!`. `src/selftest.rs` — the offline
  `self_test` capability report. `src/fiducia.rs` — read-only `fiducia_status`
  (local secret presence + endpoint probe). `src/org_map.rs` — embedded org
  map, engine docs, legacy comparison, telemetry docs (update on repo
  migration! — these back the docs:// resources too). `src/supabase.rs` —
  PostgREST read tools over the `des_client_log_*` tables (grouping is
  restricted to `GROUP_FIELDS`). `src/github.rs` — org CI status.
  `src/domains.rs` / `src/cloudflare.rs` — DoH/RDAP/TLS and the Cloudflare v4
  API. The flagship `stack_status` aggregates engine build + CI + apex DNS
  into a GREEN/DEGRADED/RED rollup (no k8s in this org).
- `supabase/schema.sql` — declarative (dpm-style) schema for the telemetry
  tables, ingest RPCs, RLS, and realtime publication. Keep the RPC/table
  names in sync with `org_map::TELEMETRY_DOCS` and the README.
- `tests/stdio_integration.rs` — hermetic MCP JSON-RPC tests against a temp
  `DES_ROOT`; they strip SUPABASE/CLOUDFLARE/FIDUCIA env vars so they never
  touch real credentials or the network. Cover tools, resources, and prompts.
- `TESTING.md` — the automated + live-wire test matrix (run the live wire
  smoke test after any change to the tool surface).

## Checks

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

Smoke-test the wire without an MCP client:

```sh
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
  | cargo run --quiet
```
