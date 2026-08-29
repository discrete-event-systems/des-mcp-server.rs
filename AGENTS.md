# des-mcp-server.rs — agent contract

## Parent / root agent contract

This file is **this repository's** agent contract. The fleet-wide parent lives at:

- GitHub: https://github.com/oresoftware/my-ai/AGENTS.md
- Disk: `~/codes/oresoftware/my-ai/AGENTS.md`
- Installed by `~/codes/oresoftware/my-ai/setup-final.sh` (not `.md`) as symlinks onto:
  - `~/codes/AGENTS.md`
  - `~/codes/.claude/AGENTS.md` and `~/codes/.claude/CLAUDE.md`
  - `~/codes/.cursor/AGENTS.md` and `~/codes/.cursor/.cursorrules`
  - `~/codes/.chatgpt/AGENTS.md`
  - `~/codes/.openai/AGENTS.md`
  - `~/codes/.anthropic/AGENTS.md`

When this file and the parent disagree: follow **this file** for this MCP
server's tools, safety boundary, and env layout; follow the parent for org-wide
git/Linear/GitHub/k8s/shared-auth/opto-sync/ores-otel/zed-pkg conventions.

The mapping is 1:1:1:1 — GitHub org : Linear project : GitHub org project
(usually `https://github.com/orgs/<org>/projects/1`) : Slack channel in
`oresoftware-workspace.slack.com`. Linear workspace: https://linear.app/denman
Primary GitHub user: `ORESoftware`. Secondary: `the1mills`.


---

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

## Syncing with the remote

"Sync with the remote" (or just "sync") is a **two-way** exchange — pull the
remote's commits down **and** push yours up. It is never push-only, and a clean
local tree does not by itself mean "synced": you are done only once local and
the remote hold the same commits.

To sync:

1. **Commit your work first** (`git add` + `git commit`) so the tree is clean —
   pull/merge only into a clean tree. `git pull` / `git merge` aborts when an
   incoming change touches a file you have edited, and even when it doesn't it
   buries the merge in your uncommitted work. (Can't commit yet? `git stash`,
   then `git stash pop` after step 3.)
2. `git fetch --all --prune` — safe any time; it only updates tracking refs.
3. `git pull` (fetch + merge) — or `git merge` the upstream branch — to
   integrate the remote's commits.
4. `git push` to publish yours.

Integrate with **`git merge` / `git pull`**. **Never `git rebase` to sync** — it
rewrites history and breaks shared branches.
