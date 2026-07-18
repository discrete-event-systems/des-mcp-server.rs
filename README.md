# des-mcp-server

An MCP (Model Context Protocol) server, in Rust, for the
**discrete-event-systems** org — the DES engine work: modeling, simulating,
solving, and rendering discrete, continuous, and mixed systems (FEL
simulation, time-stepped simulation, MDP/POMDP, LP/MILP, hybrid block
diagrams). It speaks MCP over **stdio** using the official
[`rmcp`](https://crates.io/crates/rmcp) SDK.

The engine repos currently live in **local checkouts** under the org root
(`DES_ROOT`, default `~/codes/ores`) and will migrate into
`github.com/discrete-event-systems` over time — the `org_map` tool has the
full picture. Only the DES allowlist is addressable
(`discrete-event-system.rs`, `des-engine`, `old-outmoded-engine.rs`, `des`);
the org root also holds unrelated repos, which stay invisible.

All tools are **read-only or build-only** (no deletes, no moves, no history
rewrites, no write-capable cloud tools). User-supplied values that reach a
CLI or URL (repo names, git refs, hostnames, PostgREST filters) are validated
against conservative charsets so they can't smuggle flags, shell syntax, or
query-string operators. **stdout is the MCP wire** — all logging goes to
stderr.

## Build & test

```sh
cargo test             # 44 unit tests + 10 stdio integration tests (hermetic, no network)
cargo build --release  # binary: target/release/des-mcp-server
```

See [`TESTING.md`](TESTING.md) for the full automated + live-wire test matrix.

The integration tests spawn the real binary, speak MCP JSON-RPC over stdio
against a temp org root (`DES_ROOT`), and cover happy paths plus error paths
(path traversal, allowlist enforcement, git-flag injection, PostgREST filter
injection, missing env vars).

## Register with Claude Code

```sh
claude mcp add des -- ~/codes/discrete-event-systems/des-mcp-server.rs/target/release/des-mcp-server
```

## Tools

### Org / git

| tool | what it does |
|---|---|
| `org_overview` | The DES repos under `DES_ROOT`: branch, dirty count, ahead/behind, last commit. Start here. |
| `repo_status` | One repo in detail: status, recent commits, worktrees, stashes. |
| `recent_commits` | `git log` for a repo (optional branch/count). |
| `search_code` | `git grep -E` across the DES repos — tracked files only, so `target/` and `node_modules/` are never scanned. |

### Engine builds & tests (build-only)

| tool | what it does |
|---|---|
| `cargo_check` | Fast compile check of a Rust engine repo (default `discrete-event-system.rs`); error/warning lines only. |
| `engine_tests` | `cargo test` with the engine conventions baked in: `all_targets=true` runs the CI matrix (`--all-targets --all-features`); `old-outmoded-engine.rs` defaults to lib-only. Returns the failing-test set + per-target summaries. |
| `ts_engine_check` | `npx tsc -p tsconfig.json --noEmit` in `des-engine` (the original TypeScript engine, package `uta-phd-des`). |

### Models & docs

| tool | what it does |
|---|---|
| `sim_models` | Inventory of runnable models/scenarios: des-engine's 95 JSON specs (`examples/*.json`, run via `npm run from-json`), the Rust engine's cargo examples + ~142 demo bins (`src/bin/main_*.rs`) + committed `data/`, and the legacy soccer bins. Repo + substring filters. |
| `sim_model_inspect` | **DES-unique.** Parse ONE JSON model spec without running it and report its schema, the model/citizen it drives, parameters, runtime knobs, tags, and — for the universal-math DES documents — the math payload (state variables, equations, block graph). Path validated + parse-only. |
| `engine_docs` | The engine core abstractions from the real code: FEL scheduler `Engine<W>` (`schedule_at`/`schedule_after`/`run_until`/`now`), the model-citizen contract (`CitizenRegistry`, `ModelDescriptor`, `RunArtifact`), and the TS station/tick kernel (`DESStation`, `TimeSteppedStation` — deliberately no global FEL). |
| `engine_comparison` | Legacy-vs-current: the uta-phd → des-engine (TS) → discrete-event-system.rs (Rust) lineage, and why `old-outmoded-engine.rs` (`soccer_engine`) is legacy (superseded by akrion-sim). |
| `org_map` | Org/repo map: repo locations today, migration plan, build entry points, shared ORESoftware infra (k8s GitOps, dpm, Cloudflare/Squarespace, fiducia, the MASH rule). |

### Client telemetry (Supabase, prefix `des_`)

| tool | what it does |
|---|---|
| `telemetry_docs` | The org's client→Supabase streaming pattern: `supabase.rpc('ingest_des_client_log_snapshot' / 'ingest_des_client_log_entries')` via supabase-js (WASM/TS sim visualizers) or supabase_flutter (Dart), tables, RLS, realtime, rate limits. |
| `client_log_sessions` | Recent snapshots from `des_client_log_snapshots`: session, sim id, env, entry count, trigger, user. |
| `tail_client_logs` | Entries from `des_client_log_entries` for one session, oldest→newest, optional level filter. |
| `client_log_trace` | Full ordered entry timeline for ONE session, oldest→newest, tagged with level/url/sim_id/category — the "walk this session end to end" root-cause view. |
| `client_error_summary` | Error/warn counts over a look-back window (default 24h), grouped by message or (via `group_by`) by url / sim_id / user_id / session_id / level to localize a failure to a route or model. |

The read tools need `SUPABASE_URL` + `SUPABASE_SERVICE_ROLE_KEY` (reads are
RLS-restricted to service_role; clients only ever hold the anon key, which is
INSERT-only). The declarative schema — tables, ingest RPCs, RLS, realtime
publication — lives in [`supabase/schema.sql`](supabase/schema.sql) (apply
with `dpm`, the org's declarative migration tool).

### CI

| tool | what it does |
|---|---|
| `ci_status` | Latest GitHub Actions run(s) for every repo in `github.com/discrete-event-systems`. `GITHUB_TOKEN`/`GH_TOKEN` optional. |

### Domains (Cloudflare & Squarespace)

| tool | what it does |
|---|---|
| `dns_lookup` | Resolve any domain via DNS-over-HTTPS (1.1.1.1); omit `record_type` for an A/AAAA/CNAME/NS/MX/TXT sweep. |
| `domain_info` | RDAP registration data: registrar, expiry (+days left), status, nameservers classified by DNS host, plus live NS. |
| `tls_cert_check` | Subject, issuer, `notAfter`, days-until-expiry of the cert at `host:port`. |
| `cloudflare_zones` | List zones visible to the API token. |
| `cloudflare_dns_records` | List a zone's DNS records, filtered by type/name. |

`cloudflare_*` needs `CLOUDFLARE_API_TOKEN` — create a **read-only** token
(Zone:Read + DNS:Read). Squarespace holds registration for most org domains
and has no public DNS API, so those are covered by the registrar-agnostic
tools (`domain_info` / `dns_lookup`).

### Readiness & ops

| tool | what it does |
|---|---|
| `stack_status` | **Flagship aggregate.** Runs the engine build check + latest GitHub Actions CI + (optional) apex DNS in one call and returns a compact **GREEN / DEGRADED / RED** rollup naming every failing check. This org has no k8s workload, so engine build/test health stands in for deploy status. |
| `self_test` | Offline capability report: which env vars/creds are present (values **never** shown) and therefore which tool families are LIVE vs DEGRADED, plus which DES repos are on disk. The fast "what is configured" check. |
| `fiducia_status` | Read-only [fiducia.cloud](https://fiducia.cloud) (shared secrets/locks plane) status: which required secrets are present locally, and — when `FIDUCIA_URL` + `FIDUCIA_TOKEN` are set — a health/lease endpoint probe (10s timeout, graceful "unreachable" if down). |

## Resources

Read without a tool call via the MCP `resources/list` + `resources/read` API:

| uri | what it is |
|---|---|
| `orgmap://discrete-event-systems` | The org/repo map (same content as `org_map`). |
| `docs://engine` | Engine core abstractions (FEL `Engine<W>`, model citizens, TS station/tick kernel). |
| `docs://engine-comparison` | The uta-phd → des-engine (TS) → discrete-event-system.rs (Rust) lineage. |
| `docs://telemetry` | The client→Supabase telemetry streaming pattern. |
| `schema://telemetry` | The declarative `supabase/schema.sql` (tables, ingest RPCs, RLS, realtime). |

## Prompts

Canned, parameterized workflows via `prompts/list` + `prompts/get`:

| prompt | args | what it steers |
|---|---|---|
| `engine_readiness` | — | Assess the engine's shippability via `stack_status` / `cargo_check` / `engine_tests` / `ci_status` (calls out the ~164 pre-existing legacy failures — use set-difference vs a baseline). |
| `triage_client_errors` | `hours?`, `environment?` | Triage what is breaking in the sim visualizers via `client_error_summary` (grouped by url/sim_id) + `client_log_trace`. |
| `domain_audit` | `domain` | Audit a domain via `domain_info` + `dns_lookup` + `tls_cert_check`. |

## Environment variables

| var | default | used by |
|---|---|---|
| `DES_ROOT` | `~/codes/ores` | org/git, builds, `sim_models` |
| `SUPABASE_URL` | — (required for telemetry reads) | `client_log_sessions`, `tail_client_logs`, `client_error_summary` |
| `SUPABASE_SERVICE_ROLE_KEY` | — (required for telemetry reads) | same |
| `CLOUDFLARE_API_TOKEN` | — (required for `cloudflare_*`) | `cloudflare_zones`, `cloudflare_dns_records` |
| `GITHUB_TOKEN` / `GH_TOKEN` | — (optional) | `ci_status` |
