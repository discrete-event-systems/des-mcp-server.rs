# Testing — des-mcp-server

Two layers: **automated** (`cargo test`: unit + hermetic stdio integration,
no network) and **live wire** (spawn the real release binary, speak MCP
JSON-RPC over stdio against real repos/DNS/RDAP/GitHub). Every network tool has
a timeout; a down/absent target must surface as a typed error, never a panic
or a hang.

## Automated

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

- **44 unit tests** — validation charsets (path traversal, flag/shell/PostgREST
  injection), output truncation/char-boundaries, cargo-output parsing, RDAP/DoH
  formatting, the model inventory + JSON **spec inspector**, the resource/prompt
  **catalog** (every advertised resource reads back; prompts interpolate args),
  the **self-test** report, and **fiducia** local-secret presence.
- **10 stdio integration tests** — spawn the real binary against a hermetic
  temp `DES_ROOT` (scratch git repos), covering: `tools/list` (all 26 tools),
  org/git tools, `sim_models`, `sim_model_inspect` (+ traversal/non-json
  rejection), embedded docs, **resources/prompts** (`resources/list|read`,
  `prompts/list|get`, unknown-uri/prompt → protocol error), error paths
  (traversal, allowlist, git-flag injection, PostgREST filter injection, bad
  hostname/level/group_by), missing-env typed errors, `cargo_check`, and the
  offline **ops tools** (`self_test`, `fiducia_status`).

Env vars (`SUPABASE_*`, `CLOUDFLARE_API_TOKEN`, `FIDUCIA_*`) are stripped in the
test harness so tests never touch real credentials or the network.

## Live wire matrix (release binary, real targets)

Handshake: `initialize` → `notifications/initialized` → `tools/list`. Server
advertises `tools`, `resources`, and `prompts` capabilities; all 26 tools carry
a valid input JSON Schema.

| family | tool(s) | target | result |
|---|---|---|---|
| org/git | `org_overview`, `repo_status`, `search_code` | real `~/codes/ores` DES repos | PASS |
| builds | `cargo_check` (via `stack_status`) | real `discrete-event-system.rs` | PASS — GREEN (compiles) |
| models | `sim_models`, `sim_model_inspect` | real des-engine `examples/*.json` (incl. universal-math spec) | PASS |
| domains | `dns_lookup`, `domain_info`, `tls_cert_check` | real DNS/RDAP/TLS for github.com | PASS |
| CI | `ci_status` | live GitHub API (org has no repos yet) | PASS — graceful "no repos visible" |
| telemetry | `client_log_sessions`, `client_log_trace`, `client_error_summary` | no `SUPABASE_*` set | PASS — typed, actionable error |
| domains (cf) | `cloudflare_zones` | no `CLOUDFLARE_API_TOKEN` | PASS — typed error naming the var |
| ops | `self_test` | offline | PASS — capability report, secret **presence** only |
| ops | `fiducia_status` | `FIDUCIA_*` unset | PASS — secrets report + "not configured" note (no crash) |
| ops | `stack_status` | engine build + CI + apex DNS | PASS — compact GREEN/DEGRADED/RED rollup |
| resources | `resources/read schema://telemetry` | embedded schema.sql | PASS — real SQL returned |
| prompts | `prompts/get engine_readiness` / `domain_audit` | — | PASS — steps + arg interpolation |

### Injection / error paths (all rejected before any I/O)

`repo=../../etc` (traversal) · `repo=k8s-cluster` (non-allowlist) ·
`branch=--output=/tmp/x` (git flag smuggle) · `session_id=x)&select=*`
(PostgREST operator smuggle) · `domain=a;b.com` (shell/DNS) ·
`file=../../etc/passwd` (spec traversal) · `group_by=drop table` (grouping
allowlist). After the full error barrage the server stays responsive
(`org_overview` still returns) — **no wedge, no panic, no hang**.

## Findings

No panics, hangs, or injection escapes were found. One doc/robustness item was
addressed while adding features: `client_error_summary` now validates its new
`group_by` against a fixed field allowlist (`supabase::GROUP_FIELDS`) so a
column name can never be smuggled into the PostgREST `select=`/grouping.

## Engine-repo testing note

The DES engine repos have **~164 pre-existing legacy test failures**
(old-outmoded-engine.rs). When using `engine_tests` to judge a change, compare
the **failing-test set** against a baseline (set difference) — never raw pass/
fail counts. `stack_status` and the `engine_readiness` prompt call this out.
