//! libsummit — the embeddable Summit node.
//!
//! Everything a peer needs (multicast discovery, Noise_XX sessions, chunk
//! transport, services, local REST API) lives here so it can run inside any
//! host: the `summitd` daemon, a desktop app, or an Android service.
//!
//! ```no_run
//! # async fn demo() -> anyhow::Result<()> {
//! use libsummit::{Node, NodeOptions};
//! use summit_core::config::SummitConfig;
//!
//! let node = Node::start(NodeOptions::from_env("wlan0", SummitConfig::load()?)).await?;
//! let stop = node.shutdown_handle(); // call stop.shutdown() from anywhere
//! node.run().await?;
//! # Ok(()) }
//! ```

pub mod capability;
pub mod chunk;
pub mod delivery;
pub mod dispatch;
pub mod net;
pub mod node;
pub mod session;

pub use node::{load_or_create_keypair, Node, NodeOptions, ShutdownHandle};
