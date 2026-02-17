//! Handshake state tracking for the single session listener.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::net::Ipv6Addr;
use std::sync::Arc;
use std::time::Instant;

use tokio::net::UdpSocket;
use tokio::sync::Mutex;

use summit_core::crypto::{NoiseInitiator, ResponderPending, Session};

/// Shared handshake tracker
pub type SharedTracker = Arc<Mutex<HandshakeTracker>>;

/// Tracks in-progress handshakes from multiple peers.
pub struct HandshakeTracker {
    initiators:         HashMap<Ipv6Addr, InitiatorState>,
    responders:         HashMap<Ipv6Addr, ResponderState>,
    initiators_waiting: HashMap<Ipv6Addr, InitiatorWaiting>,
    responders_waiting: HashMap<Ipv6Addr, ResponderWaiting>,
}

pub struct InitiatorState {
    pub noise: NoiseInitiator,
    pub started_at: Instant,
    pub chunk_socket: Arc<UdpSocket>,
    pub chunk_socket_port: u16,
}

pub struct ResponderState {
    pub pending: ResponderPending,
    pub started_at: Instant,
    pub chunk_socket: Arc<UdpSocket>,
    pub chunk_socket_port: u16,
}

pub struct InitiatorWaiting {
    pub session: Session,
    pub chunk_socket: Arc<UdpSocket>,
    pub chunk_socket_port: u16,
}

pub struct ResponderWaiting {
    pub session: Session,
    pub chunk_socket: Arc<UdpSocket>,
    pub local_chunk_port: u16,
}

impl HandshakeTracker {
    pub fn new() -> Self {
        Self {
            initiators: HashMap::new(),
            responders: HashMap::new(),
            initiators_waiting: HashMap::new(),
            responders_waiting: HashMap::new(),
        }
    }

    pub fn shared() -> SharedTracker {
        Arc::new(Mutex::new(Self::new()))
    }

    pub fn add_initiator(
        &mut self,
        peer_ip: Ipv6Addr,
        noise: NoiseInitiator,
        chunk_socket: Arc<UdpSocket>,
        chunk_port: u16,
    ) {
        self.initiators.insert(peer_ip, InitiatorState {
            noise,
            started_at: Instant::now(),
                               chunk_socket,
                               chunk_socket_port: chunk_port,
        });
    }

    pub fn add_responder(
        &mut self,
        peer_ip: Ipv6Addr,
        pending: ResponderPending,
        chunk_port: u16,
        chunk_socket: Arc<UdpSocket>,
    ) {
        self.responders.insert(peer_ip, ResponderState {
            pending,
            started_at: Instant::now(),
                               chunk_socket,
                               chunk_socket_port: chunk_port,
        });
    }

    pub fn add_initiator_waiting_chunk(
        &mut self,
        peer_ip: Ipv6Addr,
        session: Session,
        chunk_socket: Arc<UdpSocket>,
        chunk_port: u16,
    ) {
        self.initiators_waiting.insert(peer_ip, InitiatorWaiting {
            session,
            chunk_socket,
            chunk_socket_port: chunk_port,
        });
    }

    pub fn add_responder_waiting_chunk(
        &mut self,
        peer_ip: Ipv6Addr,
        session: Session,
        chunk_socket: Arc<UdpSocket>,
        local_chunk_port: u16,
    ) {
        self.responders_waiting.insert(peer_ip, ResponderWaiting {
            session,
            chunk_socket,
            local_chunk_port,
        });
    }

    pub fn remove_initiator(&mut self, peer_ip: &Ipv6Addr) -> Option<InitiatorState> {
        self.initiators.remove(peer_ip)
    }

    pub fn remove_responder(&mut self, peer_ip: &Ipv6Addr) -> Option<ResponderState> {
        self.responders.remove(peer_ip)
    }

    pub fn remove_initiator_waiting(&mut self, peer_ip: &Ipv6Addr) -> Option<InitiatorWaiting> {
        self.initiators_waiting.remove(peer_ip)
    }

    pub fn remove_responder_waiting(&mut self, peer_ip: &Ipv6Addr) -> Option<ResponderWaiting> {
        self.responders_waiting.remove(peer_ip)
    }

    pub fn has_responder(&self, peer_ip: &Ipv6Addr) -> bool {
        self.responders.contains_key(peer_ip)
    }

    pub fn has_responder_waiting(&self, peer_ip: &Ipv6Addr) -> bool {
        self.responders_waiting.contains_key(peer_ip)
    }

    pub fn has_initiator(&self, peer_ip: &Ipv6Addr) -> bool {
        self.initiators.contains_key(peer_ip)
    }

    pub fn has_initiator_waiting(&self, peer_ip: &Ipv6Addr) -> bool {
        self.initiators_waiting.contains_key(peer_ip)
    }

    /// Clean up stale handshakes (older than 10 seconds)
    pub fn cleanup_stale(&mut self) {
        let cutoff = Instant::now() - std::time::Duration::from_secs(10);
        self.initiators.retain(|_, state| state.started_at > cutoff);
        self.responders.retain(|_, state| state.started_at > cutoff);
    }
}
