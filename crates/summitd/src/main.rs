//! summitd — Summit peer-to-peer daemon.

mod capability;

use std::sync::Arc;
use anyhow::Result;

use summit_core::wire::{CapabilityAnnouncement, Contract};
use capability::{broadcast, listener, new_registry};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialise tracing — RUST_LOG controls verbosity
    // e.g. RUST_LOG=debug cargo run -p summitd
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // For now: read interface from first argument, default to veth-a
    let interface = std::env::args().nth(1).unwrap_or_else(|| "veth-a".to_string());

    tracing::info!(interface, "summitd starting");

    let interface_index = broadcast::if_index(&interface)?;

    // Build a dummy capability announcement for testing
    // In a real daemon this comes from config
    let test_capability = CapabilityAnnouncement {
        capability_hash: summit_core::crypto::hash(b"summit.test.ping"),
        public_key:      [0u8; 32],
        version:         1,
        session_port:    9001,
        contract:        Contract::Bulk as u8,
        flags:           0,
    };

    let capabilities = Arc::new(vec![test_capability]);
    let registry = new_registry();

    // Spawn broadcast, listener, and expiry tasks
    let broadcast_task = tokio::spawn(broadcast::broadcast_loop(
        capabilities.clone(),
        interface_index,
    ));

    let listener_task = tokio::spawn(listener::listener_loop(
        registry.clone(),
        interface_index,
    ));

    let expiry_task = tokio::spawn(listener::expiry_loop(
        registry.clone(),
    ));

    // Print registry contents every 5 seconds so we can see it working
    let registry_printer = {
        let registry = registry.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(
                std::time::Duration::from_secs(5)
            );
            loop {
                interval.tick().await;
                tracing::info!(peers = registry.len(), "registry snapshot");
                for entry in registry.iter() {
                    tracing::info!(
                        capability = hex::encode(entry.key()),
                        addr = %entry.addr,
                        "  peer"
                    );
                }
            }
        })
    };

    // Wait for any task to finish (they run forever, so this catches panics)
    tokio::select! {
        r = broadcast_task  => tracing::error!("broadcast task exited: {:?}", r),
        r = listener_task   => tracing::error!("listener task exited: {:?}", r),
        r = expiry_task     => tracing::error!("expiry task exited: {:?}", r),
        r = registry_printer => tracing::error!("printer task exited: {:?}", r),
    }

    Ok(())
}
