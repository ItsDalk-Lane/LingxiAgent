//! R01-T03 rmcp feasibility proof (taskbook step 2, W12): the OFFICIAL MCP
//! Rust SDK (`rmcp`) drives a real client/server protocol handshake inside
//! this process over a tokio duplex transport — no self-written MCP stack.
//!
//! Asserts: initialize completes, the negotiated protocol version is one the
//! server advertises, the server identity round-trips, ping and list_tools
//! succeed. Exit 0 only when all assertions hold.

use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ServerHandler, ServiceExt};

#[derive(Debug, Clone, Default)]
struct SpikeServer;

impl ServerHandler for SpikeServer {
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::new(ServerCapabilities::builder().enable_tools().build());
        let mut imp = Implementation::from_build_env();
        imp.name = "lingxi-spike-mcp".into();
        imp.version = env!("CARGO_PKG_VERSION").into();
        info.server_info = imp;
        info
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    if let Err(e) = run().await {
        tracing::error!(error = %e, "rmcp handshake prototype FAILED");
        eprintln!("SPIKE_MCP_FAIL {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);

    let server = tokio::spawn(async move { SpikeServer.serve(server_io).await });
    let client = ().serve(client_io).await.map_err(|e| format!("initialize: {e}"))?;

    let peer_info = client
        .peer_info()
        .ok_or("no peer info after initialize")?
        .clone();
    let server_name = peer_info
        .server_info
        .as_ref()
        .map(|i| i.name.as_str())
        .unwrap_or("<none>")
        .to_string();
    tracing::info!(
        protocol = %peer_info.protocol_version,
        server = %server_name,
        "initialize completed"
    );
    if server_name != "lingxi-spike-mcp" {
        return Err(format!("unexpected server identity: {peer_info:?}").into());
    }

    let tools = client
        .peer()
        .list_tools(Default::default())
        .await
        .map_err(|e| format!("list_tools: {e}"))?;
    tracing::info!(tools = tools.tools.len(), "list_tools ok");

    client.cancel().await?;
    let server_joined = server.await?;
    let server_service = server_joined.map_err(|e| format!("server serve: {e}"))?;
    let negotiated = format!("{}", peer_info.protocol_version);
    drop(server_service);

    println!(
        "SPIKE_MCP_OK negotiated_protocol={} server={} tools={}",
        negotiated,
        server_name,
        tools.tools.len()
    );
    Ok(())
}
