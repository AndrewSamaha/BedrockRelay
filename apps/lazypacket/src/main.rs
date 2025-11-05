mod proxy;
mod session;
mod packet_logger;
mod protocol;

use anyhow::Result;
use tracing::{info, error};
use proxy::ProxyServer;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    // Default to INFO level if RUST_LOG is not set
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .init();

    info!("Starting Bedrock Rust Proxy...");

    // Configuration - in the future, this could come from config file or CLI args
    let proxy = ProxyServer::new(
        "0.0.0.0:19332".parse()?,
        "192.168.1.100:19132".parse()?,
    )?;

    if let Err(e) = proxy.run().await {
        error!("Proxy server error: {}", e);
        return Err(e);
    }

    Ok(())
}
