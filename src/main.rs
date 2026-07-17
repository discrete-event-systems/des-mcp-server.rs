//! Binary entry point: serve the DES MCP tools over stdio.
//! stdout is the MCP wire — anything human-facing goes to stderr.

use des_mcp_server::server::DesMcp;
use rmcp::{ServiceExt, transport::stdio};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server = DesMcp::new();
    eprintln!(
        "des-mcp-server serving on stdio (org root: {})",
        server.root.display()
    );
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
