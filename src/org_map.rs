//! Embedded, offline reference docs for the discrete-event-systems org:
//! the org/repo map, the DES engine-core docs (event queue / scheduler /
//! clock abstractions drawn from the real code), the legacy-vs-current
//! engine comparison, and the client-telemetry-to-Supabase pattern.
//! Update these when repos migrate into github.com/discrete-event-systems.

pub const ORG_MAP: &str = r#"# discrete-event-systems — org / repo map

The org is home to the DES (Discrete Event System) engine work: modeling,
simulating, solving, and rendering discrete, continuous, and mixed systems
(FEL simulation, time-stepped simulation, MDP/POMDP, LP/MILP, hybrid block
diagrams).

## Repo locations — IMPORTANT

GitHub org github.com/discrete-event-systems currently holds:

- des-mcp-server.rs — this read-only/build-only stdio MCP server.
- des-web.rs — the dynamic MASH application server described below.
- discrete-event-systems.github.io — the public Astro organization site,
  deployed through GitHub Actions to https://discrete-event-systems.github.io.
  It has separate `/simulations/` and `/games/` galleries and embeds only
  first-party, self-contained artifacts in sandboxed browser frames.

The engine code still lives in local checkouts under the org root (default
~/codes/ores, override with DES_ROOT) and can migrate into the org over time:

- discrete-event-system.rs — CURRENT Rust engine. Crate `des_engine`
  (edition 2021, rlib/cdylib/staticlib). JSON-first SDK: machine-readable
  JSON results, JSONL frame streams, self-contained HTML players. Solver
  families: FEL sim, time-stepped sim, MDP, POMDP, LP/MILP (Clarabel
  interior-point), hybrid/acausal block diagrams. ~142 runnable demo bins
  under src/bin/ (main_elevator, main_traffic, main_epidemic, …) plus
  cargo examples under examples/. Also a SECONDARY submodule of
  ORESoftware/k8s-cluster at remote/submodules/discrete-event-system.rs —
  the standalone checkout is canonical.
- des-engine — the ORIGINAL TypeScript engine (package `uta-phd-des`,
  UTA PhD course project). Fixed-time-step, station-local kernel with NO
  global future event list. 95 JSON model specs under examples/*.json,
  run via `npm run from-json examples/<file>.json`.
- old-outmoded-engine.rs — LEGACY. Actually `soccer_engine`
  (soccer-sim-game-engine.rs): the 2D soccer sim/RL domain extracted from
  the engine, depending on des_engine by path. Outmoded locally — the
  canonical soccer engine now lives in ~/codes/akrion-sim (akrion-sim org).
  Keep for reference; do not extend.
- des — uta-phd coursework monorepo (package alex_phd), the academic
  origin of the DES entity/station vocabulary. Focus subtree:
  courses/hdm-fall-2022/des. Mostly research material, not a build target.
- des-web.rs (~/codes/discrete-event-systems/des-web.rs) — this org's MASH
  web server (maud + axum + Supabase GoTrue + SeaORM, NOT sqlx + htmx),
  serving DES sim/game pages COPIED (not ripped out) from
  ORESoftware/k8s-cluster — the originals keep running in-cluster. Routes:
  `/` sim catalog, `/soccer` tournaments + `/soccer/planner` (proxies to a
  live des-rs via DES_UPSTREAM_URL), `/routing` an in-process VRP/TSP
  dashboard, `/track3t` and `/elevator/player` self-contained vendored
  artifact players, `/elevator` a live FEL dispatch-learning UI. Schema is
  the pg-defs contract (ORESoftware/k8s-libs-and-shared-defs, vendored as
  the private `libs/` submodule) plus a des-web overlay
  (schema/des-web.sql: des_web_sims, des_web_routing_solves), migrated with
  dpm like every other org service (scripts/dpm.sh diff|verify|review|apply).

## Build / test entry points

- discrete-event-systems.github.io: `npm ci`, `npm run build` (Astro check +
  static build); GitHub Pages deploys only from `main`.
- des-mcp-server.rs: `cargo fmt --check`, clippy with warnings denied,
  `cargo test`, `cargo build --release`.
- des-web.rs: `cargo fmt --check`, clippy with warnings denied,
  `cargo build --locked`, `cargo test --locked`; schema CI applies pg-defs +
  the overlay + seed to an ephemeral Postgres 17 database.
- discrete-event-system.rs: `cargo build --release`,
  `cargo test --all-targets --all-features`, clippy -D warnings, fmt.
- des-engine: `npm run build` (tsc); per-domain `npm run test-*` /
  `npm run validate-*` scripts (ts-node).
- old-outmoded-engine.rs: `cargo test --lib` (legacy; read-only).

## Shared infra (ORESoftware side, until migration completes)

- k8s runtime: github.com/ORESoftware/k8s-cluster (~/codes/ores/k8s-cluster)
  — GitOps repo for the EC2 Kubernetes runtime synced by Argo CD. Apps under
  remote/argocd/, service source under remote/deployments/ (submodules are
  SECONDARY checkouts; upstream repos are canonical). Runtime manifests
  target branch `dev` of git@github.com:ORESoftware/k8s-cluster.git.
- Shared defs: github.com/ORESoftware/k8s-libs-and-shared-defs
  (~/codes/ores/k8s-libs-and-shared-defs).
- Migrations: org standard is `dpm` (github.com/declarative-migrations) —
  declarative, stateless, ORM-agnostic Postgres schema migration; no tracked
  migration files. Known bug: dpm never converges (deparse fixed-point) on
  varchar IN-list CHECK constraints, so libs CI runs `dpm verify` as
  advisory until fixed upstream.
- Web/DNS: Cloudflare proxies all org websites (account
  alexander.d.mills@gmail.com). Marketing sites on GitHub Pages; app code on
  Rust web servers in the k8s cluster. Squarespace typically holds domain
  REGISTRATION; DNS goes via Cloudflare. Squarespace has no public DNS API →
  use the RDAP + DNS-over-HTTPS tools here.
- Secrets/locks: fiducia.cloud supplies secrets (synced with GitHub Actions
  secrets) and distributed locks/leases (fiducia-clients).
- Observability: container stdout/stderr is first-class telemetry
  (Promtail/Loki); no monkey-patching runtimes — explicit logging APIs only.
- HTTP rule: if a service ever serves JSON/HTML it uses MASH — maud, axum,
  Supabase, SeaORM (NOT sqlx), htmx — with Supabase auth gated to
  alexander.d.mills@gmail.com only. This org's instance is des-web.rs (see
  above). This MCP server is stdio-only and serves no HTTP.

## Client telemetry

Future web/WASM sim visualizers stream client logs to Supabase (prefix
`des_`): tables des_client_log_snapshots / des_client_log_entries, ingest
RPCs ingest_des_client_log_snapshot / ingest_des_client_log_entries. See the
telemetry_docs tool and supabase/schema.sql in this repo.
"#;

pub const ENGINE_DOCS: &str = r#"# DES engine core abstractions (from the real code)

## Current Rust engine — crate `des_engine` (discrete-event-system.rs)

Future-event-list (FEL) kernel: src/des/fel/ (engine.rs, mm1.rs,
elevator.rs, compare.rs, time_stepped_mm1.rs).

- `Engine<W>` (src/des/fel/engine.rs) — THE scheduler / event queue.
  API: `Engine::new(world)`, `now() -> f64` (simulation clock),
  `events_processed()`, `pending()`, `schedule_at(time, action)`,
  `schedule_after(delay, action)`, `run_until(end)`, `run()`.
  Events are closures scheduled at/after a simulation time; the engine pops
  the earliest pending event, advances `now()`, and executes it.
- `SystemClock` (src/des/shared/capabilities.rs) — wall-clock capability,
  distinct from the FEL simulation clock.
- Exact-decimal time: the crate uses rust_decimal / num-rational where exact
  arithmetic matters (the TS engine used mathjs BigNumber for the same
  reason).

Paradigm-neutral model contract (src/des/model/):
- `ModelDescriptor`, `ModelCitizen`, `CitizenRegistry`, `RunArtifact`;
  `with_builtins()` registers the acausal, MDP, POMDP, authoring, hybrid,
  equation, and studio citizens.
- Executives (`Executive`, `StudioExecutive`, `HybridExecutive`) route a
  model graph to the SIMPLEST engine that can run it.
- Streaming solvers live in src/des/streaming/; public surface is gathered
  in src/prelude.rs, and `des_engine::sdk::surface()` reports
  name/version/modules.

## Original TypeScript engine — package `uta-phd-des` (des-engine)

Deliberately DIFFERENT kernel: fixed-time-step, station-local, and NO
global future event list. "DES" here means Discrete Event SYSTEM (a general
substrate for simulation, control loops, and iterative algorithms — LP, GA,
MCTS, value iteration, Benders), not only simulation.

- `DESStation` (src/des/general/des-base/station.ts) — abstract station in
  the source → station → sink pattern; implements `DESRunLoopEntity`.
- `StationaryEntity<E>` (src/des/abstract/abstract.ts) and
  `AbstractMovingEntity<E,V>` (src/des/entity-moving/moving.ts).
- `Station extends TimeSteppedStation` (src/des/general/field-station.ts).
- Control blocks (des-base/control-blocks.ts): `PlantBlock`,
  `ControllerBlock`, `EstimatorBlock`, `VectorSignal`, `runClosedLoop`.
- MDP-family stations: `FiniteHorizonDPStation`, `LinearVFAStation`,
  `BeliefStateStation`, `SemiMDPAgentStation`, `RLAgentStation`, …
- Entity kinds are directory-organized: entity-source/, entity-queue/,
  entity-processing/, entity-routing/, entity-travel/, entity-sink/,
  entity-decision/, entity-moving/.
- Models are JSON specs (examples/*.json, 95 of them) run through
  `npm run from-json examples/<file>.json` (main-from-json.ts).

## Rule of thumb

Use `Engine<W>` (FEL) when event times are sparse/irregular; use the
station/tick abstractions when everything advances on a fixed step. The Rust
engine supports BOTH; the TS engine is tick-only by design.
"#;

pub const ENGINE_COMPARISON: &str = r#"# Legacy vs current engines — lineage and comparison

Lineage: des (uta-phd coursework, academic origin)
  → des-engine (TypeScript `uta-phd-des`, the original working engine)
  → discrete-event-system.rs (Rust `des_engine`, the current engine)
  → old-outmoded-engine.rs (`soccer_engine`, a LEGACY domain consumer).

## des-engine (TS, original) vs discrete-event-system.rs (Rust, current)

| aspect | des-engine (TS) | discrete-event-system.rs (Rust) |
|---|---|---|
| kernel | fixed-time-step, station-local, NO global FEL | adds a true FEL scheduler `Engine<W>` alongside ported station abstractions |
| clock | discrete tick; mathjs BigNumber for exactness | `Engine::now() -> f64` sim clock; rust_decimal/num-rational for exact arithmetic |
| models | 95 JSON specs in examples/*.json via main-from-json | Rust demos: examples/*.rs + ~142 src/bin/main_*.rs; specs mostly generated from each citizen's example_spec |
| solvers | LP, GA, MCTS, value iteration, Benders as station algorithms | full families via model citizens: MDP, POMDP, LP/MILP (Clarabel), hybrid, acausal, equation, studio |
| surface | npm scripts (ts-node) per domain | JSON-first SDK (rlib/cdylib/staticlib), `des_engine::sdk::surface()` |

The Rust port is CANONICAL for new work; the TS engine remains the readable
reference for the station/tick semantics and holds the JSON model corpus.

## old-outmoded-engine.rs (`soccer_engine`) — why it is legacy

- It is the 2D soccer sim/RL DOMAIN extracted out of the engine repo; it
  depends on `des_engine = { path = "../discrete-event-system.rs" }`.
- Its README/AGENTS.md say it outright: outmoded locally — the canonical
  soccer engine is now ~/codes/akrion-sim/akrion-soccer-engine.rs
  (akrion-sim org). The training-deploy pipeline still runs from the
  ORESoftware upstream at ref `learning`.
- Architecturally narrow: only solver is the continuous formation LP
  (`SoccerFormationLpBrain::solve_tick` via Clarabel) plus per-player MPC
  (`PlanarPointMassMpc`); no MILP/branch-and-bound, no generic citizen
  registry. Contrast the current engine's full solver families.
- Treat as read-only reference (docs/ has 25 architecture/learning notes,
  e.g. learning-architecture.md, reward-shaping-audit.md).
"#;

pub const TELEMETRY_DOCS: &str = r#"# Client telemetry → Supabase (org pattern, prefix `des_`)

Every org client (future web/WASM sim visualizers, TS/WASM via supabase-js;
Dart/Flutter clients via supabase_flutter) streams logs/telemetry/traces
DIRECTLY to Supabase for debugging client code — no app-server roundtrip.
Reference implementation: dd-next-1 (browser-log-collector.ts + migrations
029–035); this org's schema lives in supabase/schema.sql of des-mcp-server.rs.

## The pattern

1. The client keeps a ring buffer of ALL log levels (error/warn/info/debug/
   trace) plus device info (UA, browser, OS, screen, timezone, connection),
   with size caps and oldest-first eviction.
2. Preferred transport is DIRECT client → Supabase over the Supabase
   Realtime WebSocket / REST:
   - `supabase.rpc('ingest_des_client_log_snapshot', { payload })` — dump
     the whole buffer as one snapshot row (on error, unhandled rejection,
     manual trigger, or periodic flush).
   - `supabase.rpc('ingest_des_client_log_entries', { entries })` — live
     stream of individual entries, batched (~2.5s cadence, faster for
     warn/error), max 500 entries per call.
   WASM/TS clients use supabase-js; Dart/Flutter uses supabase_flutter over
   the same websocket. Fallback only: an app HTTP route.
3. Tables (RLS-protected, INSERT-only for anon/authenticated; service_role
   reads): `des_client_log_snapshots` (one row per buffer dump: session,
   env, trigger, context/device info, serialized entries + counts) and
   `des_client_log_entries` (one row per streamed entry: level, message,
   stack, url, sim_id for the simulation/model being visualized,
   client_timestamp).
4. Both tables are in the `supabase_realtime` publication, so debugging
   UIs can watch logs arrive live.
5. Rate limiting is client-side (snapshot ≥10s apart; entry-stream batch
   cadence + bounded queue) plus server-side guards (600KB snapshot payload
   cap; ≤500 entries per RPC call).

## Debugging from this MCP server (read-only, service-role)

- `client_log_sessions` — recent snapshots, filter by environment/user.
- `tail_client_logs` — entries for one session, optional level filter.
- `client_error_summary` — recent error/warn counts grouped by message.

Env: SUPABASE_URL + SUPABASE_SERVICE_ROLE_KEY (PostgREST; never ship the
service-role key to clients — clients use the anon key, which RLS restricts
to INSERT).
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn org_map_names_the_repos_and_migration() {
        for needle in [
            "discrete-event-system.rs",
            "des-engine",
            "old-outmoded-engine.rs",
            "des-mcp-server.rs",
            "discrete-event-systems.github.io",
            "/simulations/",
            "/games/",
            "uta-phd",
            "DES_ROOT",
            "migrate",
            "ORESoftware/k8s-cluster",
            "ORESoftware/k8s-libs-and-shared-defs",
            "dpm",
            "MASH",
        ] {
            assert!(ORG_MAP.contains(needle), "org map missing {needle:?}");
        }
    }

    #[test]
    fn org_map_covers_des_web_the_mash_server() {
        for needle in [
            "des-web.rs",
            "maud",
            "axum",
            "SeaORM",
            "htmx",
            "des_web_sims",
            "des_web_routing_solves",
            "scripts/dpm.sh",
        ] {
            assert!(ORG_MAP.contains(needle), "org map missing {needle:?}");
        }
    }

    #[test]
    fn engine_docs_cite_real_types_and_paths() {
        for needle in [
            "Engine<W>",
            "schedule_at",
            "run_until",
            "src/des/fel/engine.rs",
            "DESStation",
            "TimeSteppedStation",
            "CitizenRegistry",
            "future event list",
        ] {
            assert!(
                ENGINE_DOCS.to_lowercase().contains(&needle.to_lowercase()),
                "engine docs missing {needle:?}"
            );
        }
    }

    #[test]
    fn comparison_flags_legacy_and_akrion_successor() {
        for needle in [
            "soccer_engine",
            "akrion-sim",
            "legacy",
            "SoccerFormationLpBrain",
            "des_engine",
        ] {
            assert!(
                ENGINE_COMPARISON.contains(needle),
                "comparison missing {needle:?}"
            );
        }
    }

    #[test]
    fn telemetry_docs_cite_rpcs_and_tables() {
        for needle in [
            "ingest_des_client_log_snapshot",
            "ingest_des_client_log_entries",
            "des_client_log_snapshots",
            "des_client_log_entries",
            "supabase-js",
            "supabase_flutter",
            "SUPABASE_SERVICE_ROLE_KEY",
        ] {
            assert!(
                TELEMETRY_DOCS.contains(needle),
                "telemetry docs missing {needle:?}"
            );
        }
    }
}
