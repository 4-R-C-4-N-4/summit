//! One-way multicast: one node can't hear announcements, unicast still works.
//!
//! This is the normal state of an Android app without a MulticastLock (and of
//! some Wi-Fi networks). A session must still form, whichever side has the
//! lower key (the lower key is the designated initiator).

use crate::fault::*;
use crate::*;
use summit_core::crypto::Keypair;

/// Two fresh identities, returned as (lower public key, higher public key).
fn ordered_keys() -> ([u8; 32], [u8; 32]) {
    let a = Keypair::generate();
    let b = Keypair::generate();
    let (low, high) = if a.public < b.public { (a, b) } else { (b, a) };
    (*low.private_bytes(), *high.private_bytes())
}

fn public_hex(private: [u8; 32]) -> String {
    hex_encode(&Keypair::from_private(private).public)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// How `ns` knows `peer`: Some("multicast" | "handshake"), or None if unknown.
fn discovered_via(ns: &str, peer: &str) -> Option<String> {
    let peers = api_get(ns, "/peers").ok()?;
    peers["peers"]
        .as_array()?
        .iter()
        .find(|p| p["public_key"].as_str() == Some(peer))
        .and_then(|p| p["discovered_via"].as_str().map(String::from))
}

fn has_session_with(ns: &str, peer: &str) -> bool {
    api_get(ns, "/status")
        .ok()
        .and_then(|s| {
            s["sessions"]
                .as_array()
                .map(|a| a.iter().any(|x| x["peer_pubkey"].as_str() == Some(peer)))
        })
        .unwrap_or(false)
}

fn poll(timeout: Duration, mut f: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if f() {
            return true;
        }
        thread::sleep(Duration::from_millis(500));
    }
    false
}

fn message_arrives(from_ns: &str, to_ns: &str, to_key: &str, from_key: &str, text: &str) -> bool {
    let body = serde_json::json!({ "to": to_key, "text": text }).to_string();
    if api_post(from_ns, "/messages/send", &body).is_err() {
        return false;
    }
    poll(Duration::from_secs(10), || {
        api_get(to_ns, &format!("/messages/{from_key}"))
            .ok()
            .and_then(|m| {
                m["messages"].as_array().map(|a| {
                    a.iter()
                        .any(|x| x["content"]["text"].as_str() == Some(text))
                })
            })
            .unwrap_or(false)
    })
}

/// B drops inbound multicast. `deaf_has_lower_key` picks which side is the
/// designated initiator.
fn run_one_way(deaf_has_lower_key: bool) {
    if !skip_unless_ready() {
        return;
    }
    let _lock = daemon_lock();
    cleanup_summitd();

    let (low, high) = ordered_keys();
    let (key_b, key_a) = if deaf_has_lower_key {
        (low, high)
    } else {
        (high, low)
    };
    let (pub_a, pub_b) = (public_hex(key_a), public_hex(key_b));

    let _deaf = block_inbound_multicast(NS_B);
    let auto_env = [("SUMMIT_TRUST__AUTO_TRUST", "true")];
    let mut node_a = spawn_daemon_with_key(NS_A, VETH_A, &auto_env, key_a);
    let mut node_b = spawn_daemon_with_key(NS_B, VETH_B, &auto_env, key_b);

    let result = (|| -> Result<()> {
        wait_for_api(NS_A, 40)?;
        wait_for_api(NS_B, 40)?;

        // Precondition: the fault is real. A hears B over multicast; B never
        // hears A's announcements. Otherwise this test proves nothing.
        if !poll(Duration::from_secs(10), || {
            discovered_via(NS_A, &pub_b).as_deref() == Some("multicast")
        }) {
            bail!("A never heard B's announcements");
        }
        if discovered_via(NS_B, &pub_a).as_deref() == Some("multicast") {
            bail!("B heard A over multicast: the drop rule isn't working");
        }

        // Designated initiator within one tick, fallback within ~7 s + tick.
        if !poll(Duration::from_secs(25), || {
            has_session_with(NS_A, &pub_b) && has_session_with(NS_B, &pub_a)
        }) {
            bail!("no session formed with one-way multicast");
        }

        // B knows A only through the handshake.
        assert_eq!(
            discovered_via(NS_B, &pub_a).as_deref(),
            Some("handshake"),
            "B should list A as learned from the handshake"
        );

        assert!(
            message_arrives(NS_A, NS_B, &pub_b, &pub_a, "a-to-b"),
            "message A -> B not delivered"
        );
        assert!(
            message_arrives(NS_B, NS_A, &pub_a, &pub_b, "b-to-a"),
            "message B -> A not delivered"
        );

        // B never refreshes A via multicast; the entry must survive the
        // registry TTL while the session is alive.
        thread::sleep(Duration::from_secs(summit_core::wire::PEER_TTL_SECS + 3));
        assert!(
            discovered_via(NS_B, &pub_a).is_some(),
            "B dropped A from its registry despite a live session"
        );
        assert!(has_session_with(NS_B, &pub_a), "session died");
        Ok(())
    })();

    node_a.kill().ok();
    node_b.kill().ok();
    cleanup_summitd();
    result.unwrap();
}

/// The deaf side is the designated initiator but can't see whom to dial:
/// needs the higher-key side's fallback initiation.
#[test]
fn test_one_way_multicast_deaf_side_has_lower_key() {
    run_one_way(true);
}

/// The hearing side is the designated initiator: needs the deaf side to accept
/// a HandshakeInit from a peer it never heard announce.
#[test]
fn test_one_way_multicast_deaf_side_has_higher_key() {
    run_one_way(false);
}
