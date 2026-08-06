# Shared MCP infrastructure

This server delegates version-neutral bootstrap policy to
`ORESoftware/mcp-rust-libs` at exact merge commit
`a5c1ba9c50493ac625dd2fb175af21263d0d2801`.

`ore-mcp-bootstrap` owns bounded, secret-free resource-attribute classification
and validated service/namespace/stdio identity. DES retains its OpenTelemetry
0.32 provider/exporter construction, local resource ceilings and canonical
keys, every simulation MCP tool/schema, model authority, credentials, endpoint
allowlists, timeouts, and product clients.

The exact revision is recorded in `Cargo.toml` and `Cargo.lock`. Moving it
requires lockfile regeneration plus formatting, strict Clippy, all-target/
all-feature tests, release build, and the repository's normal architecture and
stdio checks.
