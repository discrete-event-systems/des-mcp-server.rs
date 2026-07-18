# Security policy

Please report suspected vulnerabilities privately through GitHub's
**Security → Report a vulnerability** flow for this repository. Do not open a
public issue with exploit details, credentials, telemetry data, or local path
information.

Supported code is the latest commit on `main`.

The MCP server is intentionally stdio-only and its tools must remain read-only
or build-only. Never add delete, history-rewrite, write-capable cloud, or
write-capable database tools without a separate operator authorization and
audit design. Secret values must never be emitted on stdout, which is reserved
for the MCP protocol.
