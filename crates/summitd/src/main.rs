//! summitd — Summit peer-to-peer daemon.
//!
//! Thin wrapper around [`libsummit::Node`]: load config, pick the interface
//! from argv, run until Ctrl-C or `/api/daemon/shutdown`.

use anyhow::Result;
use libsummit::{Node, NodeOptions};
use summit_core::config::SummitConfig;

#[tokio::main]
async fn main() -> Result<()> {
    // No colour codes when logging to a file or pipe (adb, systemd, tests):
    // they make logs unreadable and break grep.
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()))
        .init();

    if let Err(e) = SummitConfig::write_default_if_missing() {
        tracing::warn!(error = %e, "failed to write default config");
    }
    let config = SummitConfig::load().unwrap_or_else(|e| {
        tracing::warn!(error = %e, "failed to load config, using defaults");
        SummitConfig::default()
    });

    let interface = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "veth-a".to_string());

    let node = Node::start(NodeOptions::from_env(interface, config)).await?;

    let stop = node.shutdown_handle();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("shutdown signal received");
        stop.shutdown();
    });

    node.run().await
}
