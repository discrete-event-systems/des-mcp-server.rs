//! Binary entry point: serve the DES MCP tools over stdio.
//! stdout is the MCP wire — anything human-facing goes to stderr.

use des_mcp_server::server::DesMcp;
use rmcp::{ServiceExt, transport::stdio};
use tracing::Instrument;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _telemetry = des_mcp_server::telemetry::init("des-mcp-server", "discrete-event-systems");
    let server = DesMcp::new();
    tracing::info!(
        org.root = %server.root.display(),
        transport = "stdio",
        "starting MCP server"
    );
    let server_span = tracing::info_span!("mcp.server", rpc.system = "mcp", transport = "stdio");
    let service = server
        .serve(stdio())
        .instrument(server_span.clone())
        .await?;
    service.waiting().instrument(server_span).await?;
    Ok(())
}
