//! des-mcp-server — MCP server (stdio) for the discrete-event-systems org:
//! git/org navigation over the DES engine repos, cargo/tsc build-and-test
//! runners, simulation-model inventory, embedded engine/lineage docs,
//! Supabase client-telemetry debugging tools, domain tooling (Cloudflare
//! API, DoH, RDAP, TLS expiry), and GitHub Actions CI status.
//!
//! All tools are read-only or build-only (no deletes, no moves, no git
//! history rewrites, no write-capable cloud tools). stdout is the MCP wire;
//! logging goes to stderr only.

pub mod catalog;
pub mod cloudflare;
pub mod domains;
pub mod fiducia;
pub mod github;
pub mod models;
pub mod org_map;
pub mod selftest;
pub mod server;
pub mod spec;
pub mod supabase;
pub mod telemetry;
pub mod util;
