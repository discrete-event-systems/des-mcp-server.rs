//! Binary entry point: serve the DES MCP tools over stdio.
//! stdout is the MCP wire — anything human-facing goes to stderr.

use des_mcp_server::{server::DesMcp, shared_bootstrap};
use rmcp::{ServiceExt, transport::stdio};
use tracing::Instrument;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let identity = shared_bootstrap::stdio_identity()?;
    let _telemetry = des_mcp_server::telemetry::init(
        shared_bootstrap::SERVICE_NAME,
        shared_bootstrap::SERVICE_NAMESPACE,
    );
    let server = DesMcp::new();
    let resource_attribute_count = shared_bootstrap::environment_resource_attributes().len();
    tracing::info!(
        service.name = identity.service_name(),
        service.namespace = identity.service_namespace(),
        org.root = %server.root.display(),
        transport = identity.transport(),
        otel.resource_attribute_count = resource_attribute_count,
        "starting MCP server"
    );
    let server_span = tracing::info_span!(
        "mcp.server",
        rpc.system = "mcp",
        transport = identity.transport()
    );
    let service = server
        .serve(stdio())
        .instrument(server_span.clone())
        .await?;
    service.waiting().instrument(server_span).await?;
    Ok(())
}
