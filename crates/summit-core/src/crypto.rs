//! Cryptographic primitives for Summit.
//!
//! Provides two things:
//!   1. BLAKE3 hashing — content hashes, schema IDs, session ID derivation
//!   2. Noise_XX session establishment — authenticated key exchange
//!
//! Keypairs are managed via x25519-dalek for explicit key control.
//! snow drives the Noise_XX state machine using those keys.
//!
//! All key material derives ZeroizeOnDrop — wiped from memory when dropped.
//! There is no unsafe code in this module.

use rand::RngCore;
use snow::{Builder, HandshakeState, StatelessTransportState};
use thiserror::Error;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

// ── BLAKE3 ────────────────────────────────────────────────────────────────────

/// Hash a byte slice, returning a 32-byte BLAKE3 digest.
///
/// Used for content hashes, schema IDs, capability hashes,
/// and session ID derivation.
pub fn hash(data: &[u8]) -> [u8; 32] {
    *blake3::hash(data).as_bytes()
}

/// Derive a session ID from the two handshake nonces.
///
/// Neither party controls the session ID unilaterally — it requires
/// contributions from both sides of the handshake.
///
///   session_id = BLAKE3(initiator_nonce || responder_nonce)
pub fn derive_session_id(initiator_nonce: &[u8; 16], responder_nonce: &[u8; 16]) -> [u8; 32] {
    let mut combined = [0u8; 32];
    combined[..16].copy_from_slice(initiator_nonce);
    combined[16..].copy_from_slice(responder_nonce);
    hash(&combined)
}

/// Incremental BLAKE3 hasher for payloads that arrive in pieces.
///
/// # Example
/// ```
/// use summit_core::crypto::Hasher;
/// let mut h = Hasher::new();
/// h.update(b"hello ");
/// h.update(b"world");
/// let digest = h.finalize();
/// assert_eq!(digest, summit_core::crypto::hash(b"hello world"));
/// ```
pub struct Hasher(blake3::Hasher);

impl Hasher {
    pub fn new() -> Self {
        Self(blake3::Hasher::new())
    }

    pub fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    pub fn finalize(self) -> [u8; 32] {
        *self.0.finalize().as_bytes()
    }
}

impl Default for Hasher {
    fn default() -> Self {
        Self::new()
    }
}

// ── Keypair ───────────────────────────────────────────────────────────────────

/// The Noise protocol pattern Summit uses.
///
/// Noise_XX: mutual authentication, both static keys transmitted encrypted.
/// Neither key is visible to a passive observer.
const NOISE_PATTERN: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";

/// A device's long-term static X25519 keypair.
///
/// Generated once per device and stored persistently. The public key appears
/// in every CapabilityAnnouncement. The private key never leaves this struct.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct Keypair {
    /// Private key — zeroized on drop, never exposed directly.
    private: Zeroizing<[u8; 32]>,
    /// Public key — included in capability announcements.
    pub public: [u8; 32],
}

impl Keypair {
    /// Generate a new random X25519 keypair.
    pub fn generate() -> Self {
        let secret = StaticSecret::random_from_rng(rand::thread_rng());
        let public = PublicKey::from(&secret);
        Self {
            private: Zeroizing::new(secret.to_bytes()),
            public: *public.as_bytes(),
        }
    }

    /// Reconstruct a keypair from stored private key bytes.
    /// The public key is derived deterministically from the private key.
    pub fn from_private(private_bytes: [u8; 32]) -> Self {
        let secret = StaticSecret::from(private_bytes);
        let public = PublicKey::from(&secret);
        Self {
            private: Zeroizing::new(private_bytes),
            public: *public.as_bytes(),
        }
    }

    /// Serialize the private key for persistent storage.
    ///
    /// Store these bytes securely (mode 0600, ideally encrypted at rest).
    /// The public key need not be stored — it is always derived on load.
    pub fn private_bytes(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(*self.private)
    }
}

// ── Noise Handshake ───────────────────────────────────────────────────────────

/// Generate a cryptographically random 16-byte nonce.
pub fn generate_nonce() -> [u8; 16] {
    let mut nonce = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut nonce);
    nonce
}

/// Initiator side of the Noise_XX handshake.
///
/// The initiator has found a peer in the capability registry and
/// wants to open a session. It sends message 1, receives message 2,
/// and produces a completed Session.
pub struct NoiseInitiator {
    state: HandshakeState,
    initiator_nonce: [u8; 16],
}

impl NoiseInitiator {
    /// Begin a handshake as the initiator.
    ///
    /// Returns the initiator state and the bytes of message 1 to send
    /// to the responder (embedded in HandshakeInit on the wire).
    pub fn new(keypair: &Keypair) -> Result<(Self, Vec<u8>), CryptoError> {
        let state = Builder::new(NOISE_PATTERN.parse().map_err(|_| CryptoError::BadPattern)?)
            .local_private_key(&*keypair.private)
            .build_initiator()
            .map_err(CryptoError::Noise)?;

        let nonce = generate_nonce();

        let mut initiator = Self {
            state,
            initiator_nonce: nonce,
        };

        let mut msg1 = vec![0u8; 48];
        let len = initiator
            .state
            .write_message(&[], &mut msg1)
            .map_err(CryptoError::Noise)?;
        msg1.truncate(len);

        Ok((initiator, msg1))
    }

    /// The nonce generated by the initiator.
    /// Include this in the HandshakeInit struct on the wire.
    pub fn nonce(&self) -> &[u8; 16] {
        &self.initiator_nonce
    }

    /// Process the responder's message 2 and complete the handshake.
    ///
    /// `msg2` is the raw Noise message bytes from the HandshakeResponse.
    /// `responder_nonce` is taken from HandshakeResponse.nonce on the wire.
    ///
    /// On success, returns a completed Session ready for chunk exchange.
    pub fn finish(
        mut self,
        msg2: &[u8],
        responder_nonce: &[u8; 16],
    ) -> Result<(Session, Vec<u8>), CryptoError> {
        // Read message 2
        let mut payload = vec![0u8; msg2.len()];
        self.state
            .read_message(msg2, &mut payload)
            .map_err(CryptoError::Noise)?;

        // Write message 3 — initiator's encrypted static key + payload
        let mut msg3 = vec![0u8; 96];
        let len = self
            .state
            .write_message(&[], &mut msg3)
            .map_err(CryptoError::Noise)?;
        msg3.truncate(len);

        let transport = self
            .state
            .into_stateless_transport_mode()
            .map_err(CryptoError::Noise)?;
        let session_id = derive_session_id(&self.initiator_nonce, responder_nonce);

        Ok((
            Session {
                session_id,
                transport,
                send_nonce: 0,
                recv_window: ReplayWindow::new(),
            },
            msg3,
        ))
    }
}

/// Responder side of the Noise_XX handshake.
///
/// The responder is listening on its session_port and receives a
/// HandshakeInit from an initiator. It processes message 1, writes
/// message 2, and produces a completed Session.
pub struct NoiseResponder {
    state: HandshakeState,
    responder_nonce: [u8; 16],
}

impl NoiseResponder {
    /// Begin a handshake as the responder.
    pub fn new(keypair: &Keypair) -> Result<Self, CryptoError> {
        let state = Builder::new(NOISE_PATTERN.parse().map_err(|_| CryptoError::BadPattern)?)
            .local_private_key(&*keypair.private)
            .build_responder()
            .map_err(CryptoError::Noise)?;

        Ok(Self {
            state,
            responder_nonce: generate_nonce(),
        })
    }

    /// The nonce generated by the responder.
    /// Include this in the HandshakeResponse struct on the wire.
    pub fn nonce(&self) -> &[u8; 16] {
        &self.responder_nonce
    }

    /// Process the initiator's message 1 and write message 2.
    ///
    /// `msg1` is the raw Noise message bytes from HandshakeInit.
    /// `initiator_nonce` is taken from HandshakeInit.nonce on the wire.
    ///
    /// Returns the bytes of message 2 to send back, and a completed Session.
    pub fn respond(
        mut self,
        msg1: &[u8],
        initiator_nonce: &[u8; 16],
    ) -> Result<(ResponderPending, Vec<u8>), CryptoError> {
        // Read message 1
        let mut payload = vec![0u8; msg1.len()];
        self.state
            .read_message(msg1, &mut payload)
            .map_err(CryptoError::Noise)?;

        // Write message 2
        let mut msg2 = vec![0u8; 96];
        let len = self
            .state
            .write_message(&[], &mut msg2)
            .map_err(CryptoError::Noise)?;
        msg2.truncate(len);

        Ok((
            ResponderPending {
                state: self.state,
                responder_nonce: self.responder_nonce,
                initiator_nonce: *initiator_nonce,
            },
            msg2,
        ))
    }
}

/// Responder waiting for message 3 from the initiator.
pub struct ResponderPending {
    state: HandshakeState,
    responder_nonce: [u8; 16],
    initiator_nonce: [u8; 16],
}

impl ResponderPending {
    /// Read message 3 and complete the handshake.
    pub fn finish(mut self, msg3: &[u8]) -> Result<Session, CryptoError> {
        let mut payload = vec![0u8; msg3.len()];
        self.state
            .read_message(msg3, &mut payload)
            .map_err(CryptoError::Noise)?;

        let transport = self
            .state
            .into_stateless_transport_mode()
            .map_err(CryptoError::Noise)?;
        let session_id = derive_session_id(&self.initiator_nonce, &self.responder_nonce);

        Ok(Session {
            session_id,
            transport,
            send_nonce: 0,
            recv_window: ReplayWindow::new(),
        })
    }
}

// ── Replay Window ─────────────────────────────────────────────────────────────

/// Sliding-window replay protection (RFC 6479 style).
///
/// Tracks the highest seen nonce and a bitmap of the last 2048 nonces.
/// Rejects duplicates and nonces that fall behind the window.
const WINDOW_SIZE: u64 = 2048;

pub struct ReplayWindow {
    highest: u64,
    bitmap: Vec<u64>, // 2048 bits = 32 u64s
}

impl Default for ReplayWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl ReplayWindow {
    pub fn new() -> Self {
        Self {
            highest: 0,
            bitmap: vec![0u64; (WINDOW_SIZE / 64) as usize],
        }
    }

    /// Returns true if the nonce is acceptable (not replayed, not too old).
    pub fn check(&self, nonce: u64) -> bool {
        if nonce + WINDOW_SIZE < self.highest {
            return false; // too old
        }
        if nonce > self.highest {
            return true; // ahead of window
        }
        let diff = self.highest - nonce;
        let (word, bit) = ((diff / 64) as usize, (diff % 64) as u32);
        self.bitmap[word] & (1u64 << bit) == 0
    }

    /// Mark a nonce as seen. Call after successful decrypt.
    pub fn mark(&mut self, nonce: u64) {
        if nonce > self.highest {
            let shift = nonce - self.highest;
            self.shift_window(shift);
            self.highest = nonce;
        }
        let diff = self.highest - nonce;
        let (word, bit) = ((diff / 64) as usize, (diff % 64) as u32);
        self.bitmap[word] |= 1u64 << bit;
    }

    fn shift_window(&mut self, shift: u64) {
        if shift >= WINDOW_SIZE {
            self.bitmap.fill(0);
            return;
        }
        let word_shift = (shift / 64) as usize;
        let bit_shift = (shift % 64) as u32;
        if word_shift > 0 {
            self.bitmap.rotate_right(word_shift);
            for w in &mut self.bitmap[..word_shift] {
                *w = 0;
            }
        }
        if bit_shift > 0 {
            let len = self.bitmap.len();
            for i in (1..len).rev() {
                self.bitmap[i] =
                    (self.bitmap[i] >> bit_shift) | (self.bitmap[i - 1] << (64 - bit_shift));
            }
            self.bitmap[0] >>= bit_shift;
        }
    }
}

// ── Session ───────────────────────────────────────────────────────────────────

/// A completed Noise_XX session, ready for chunk encryption and decryption.
///
/// Uses StatelessTransportState with explicit nonces — correct for UDP where
/// packets may be lost, reordered, or retransmitted. Each encrypted packet
/// carries an 8-byte LE nonce prefix on the wire.
///
/// Wire format per packet:
///   [u64 nonce LE (8 bytes)] [Noise ciphertext (payload + 16-byte MAC)]
///
/// Session is NOT Sync — send_nonce and recv_window require exclusive access.
/// Wrap in Arc<Mutex<Session>> for shared use across tasks.
pub struct Session {
    pub session_id: [u8; 32],
    transport: StatelessTransportState,
    send_nonce: u64,
    recv_window: ReplayWindow,
}

impl Session {
    /// Encrypt plaintext into `out`. Prepends an 8-byte LE nonce and appends
    /// a 16-byte Poly1305 MAC.
    ///
    /// `out` will be `8 + plaintext.len() + 16` bytes on success.
    pub fn encrypt(&mut self, plaintext: &[u8], out: &mut Vec<u8>) -> Result<(), CryptoError> {
        let nonce = self.send_nonce;
        self.send_nonce += 1;

        out.clear();
        out.extend_from_slice(&nonce.to_le_bytes());

        let offset = 8;
        out.resize(offset + plaintext.len() + 16, 0);
        let written = self
            .transport
            .write_message(nonce, plaintext, &mut out[offset..])
            .map_err(CryptoError::Noise)?;
        out.truncate(offset + written);
        Ok(())
    }

    /// Decrypt ciphertext into `out`. Reads the 8-byte LE nonce prefix,
    /// checks the replay window, and verifies the Poly1305 MAC.
    ///
    /// Returns Err on replay, truncation, or MAC failure.
    pub fn decrypt(&mut self, ciphertext: &[u8], out: &mut Vec<u8>) -> Result<(), CryptoError> {
        if ciphertext.len() < 8 + 16 {
            return Err(CryptoError::TooShort);
        }

        let nonce = u64::from_le_bytes(ciphertext[..8].try_into().unwrap());

        if !self.recv_window.check(nonce) {
            return Err(CryptoError::Replay);
        }

        out.resize(ciphertext.len() - 8, 0);
        let written = self
            .transport
            .read_message(nonce, &ciphertext[8..], out)
            .map_err(CryptoError::Noise)?;
        out.truncate(written);

        self.recv_window.mark(nonce);
        Ok(())
    }
}

// ── Errors ────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("invalid Noise pattern string — this is a bug")]
    BadPattern,

    #[error("Noise protocol error: {0}")]
    Noise(#[from] snow::Error),

    #[error("ciphertext too short (need at least 24 bytes: 8 nonce + 16 MAC)")]
    TooShort,

    #[error("replayed or too-old nonce")]
    Replay,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: run a complete handshake and return both sessions ready for use.
    fn completed_sessions() -> (Session, Session) {
        let ikp = Keypair::generate();
        let rkp = Keypair::generate();

        // Message 1: initiator -> responder
        let (initiator, msg1) = NoiseInitiator::new(&ikp).unwrap();
        let i_nonce = *initiator.nonce();

        // Message 2: responder -> initiator
        let responder = NoiseResponder::new(&rkp).unwrap();
        let r_nonce = *responder.nonce();
        let (pending, msg2) = responder.respond(&msg1, &i_nonce).unwrap();

        // Message 3: initiator -> responder
        let (i_session, msg3) = initiator.finish(&msg2, &r_nonce).unwrap();

        // Responder completes
        let r_session = pending.finish(&msg3).unwrap();

        (i_session, r_session)
    }

    // ── BLAKE3 ────────────────────────────────────────────────────────────────

    #[test]
    fn hash_known_vector() {
        // BLAKE3 official test vector for the empty input
        let expected = [
            0xaf, 0x13, 0x49, 0xb9, 0xf5, 0xf9, 0xa1, 0xa6, 0xa0, 0x40, 0x4d, 0xea, 0x36, 0xdc,
            0xc9, 0x49, 0x9b, 0xcb, 0x25, 0xc9, 0xad, 0xc1, 0x12, 0xb7, 0xcc, 0x9a, 0x93, 0xca,
            0xe4, 0x1f, 0x32, 0x62,
        ];
        assert_eq!(hash(b""), expected);
    }

    #[test]
    fn hash_is_deterministic() {
        assert_eq!(hash(b"summit"), hash(b"summit"));
        assert_ne!(hash(b"summit"), hash(b"Summit"));
    }

    #[test]
    fn incremental_hasher_matches_oneshot() {
        let mut h = Hasher::new();
        h.update(b"hello ");
        h.update(b"world");
        assert_eq!(h.finalize(), hash(b"hello world"));
    }

    #[test]
    fn session_id_uses_both_nonces() {
        let n1 = [0x01u8; 16];
        let n2 = [0x02u8; 16];
        let id_ab = derive_session_id(&n1, &n2);
        let id_ba = derive_session_id(&n2, &n1);
        // Order matters — initiator and responder must agree on who is who
        assert_ne!(id_ab, id_ba);
    }

    #[test]
    fn session_id_is_deterministic() {
        let n1 = [0xaau8; 16];
        let n2 = [0xbbu8; 16];
        assert_eq!(derive_session_id(&n1, &n2), derive_session_id(&n1, &n2));
    }

    // ── Keypair ───────────────────────────────────────────────────────────────

    #[test]
    fn keypair_generate_produces_valid_pair() {
        let kp = Keypair::generate();
        // Public key must not be all zeros (astronomically unlikely with valid generation)
        assert_ne!(kp.public, [0u8; 32]);
    }

    #[test]
    fn keypair_roundtrip_via_private_bytes() {
        let kp1 = Keypair::generate();
        let private = kp1.private_bytes();
        let kp2 = Keypair::from_private(*private);
        // Same private key must produce same public key
        assert_eq!(kp1.public, kp2.public);
    }

    #[test]
    fn two_keypairs_are_different() {
        let kp1 = Keypair::generate();
        let kp2 = Keypair::generate();
        assert_ne!(kp1.public, kp2.public);
    }

    // ── Noise_XX Handshake ────────────────────────────────────────────────────

    #[test]
    fn print_wire_struct_sizes() {
        use crate::wire::*;
        println!(
            "HandshakeInit wire size:     {}",
            std::mem::size_of::<HandshakeInit>()
        );
        println!(
            "HandshakeResponse wire size: {}",
            std::mem::size_of::<HandshakeResponse>()
        );
        println!(
            "HandshakeComplete wire size: {}",
            std::mem::size_of::<HandshakeComplete>()
        );
        println!("Expected noise msg1 size: 32");
        println!("Expected noise msg2 size: 96");
        println!("Expected noise msg3 size: 64");
    }

    #[test]
    fn print_noise_message_sizes() {
        let ikp = Keypair::generate();
        let rkp = Keypair::generate();

        let (initiator, msg1) = NoiseInitiator::new(&ikp).unwrap();
        let i_nonce = *initiator.nonce();
        println!("msg1 size: {}", msg1.len());

        let responder = NoiseResponder::new(&rkp).unwrap();
        let r_nonce = *responder.nonce();
        let (pending, msg2) = responder.respond(&msg1, &i_nonce).unwrap();
        println!("msg2 size: {}", msg2.len());

        let (_session, msg3) = initiator.finish(&msg2, &r_nonce).unwrap();
        println!("msg3 size: {}", msg3.len());
    }

    #[test]
    fn noise_handshake_completes() {
        let (i_session, r_session) = completed_sessions();
        assert_eq!(i_session.session_id, r_session.session_id);
    }

    #[test]
    fn noise_session_encrypt_decrypt_roundtrip() {
        let (mut initiator_session, mut responder_session) = completed_sessions();

        let plaintext = b"hello from initiator";
        let mut ciphertext = Vec::new();
        let mut recovered = Vec::new();

        initiator_session
            .encrypt(plaintext, &mut ciphertext)
            .unwrap();

        // Ciphertext must be longer than plaintext (MAC appended)
        assert!(ciphertext.len() > plaintext.len());
        // Ciphertext must not equal plaintext
        assert_ne!(ciphertext.as_slice(), plaintext.as_slice());

        responder_session
            .decrypt(&ciphertext, &mut recovered)
            .unwrap();
        assert_eq!(recovered.as_slice(), plaintext.as_slice());
    }

    #[test]
    fn noise_session_both_directions() {
        let (mut initiator_session, mut responder_session) = completed_sessions();

        // Initiator -> Responder
        let mut ct = Vec::new();
        let mut pt = Vec::new();
        initiator_session.encrypt(b"ping", &mut ct).unwrap();
        responder_session.decrypt(&ct, &mut pt).unwrap();
        assert_eq!(pt, b"ping");

        // Responder -> Initiator
        let mut ct2 = Vec::new();
        let mut pt2 = Vec::new();
        responder_session.encrypt(b"pong", &mut ct2).unwrap();
        initiator_session.decrypt(&ct2, &mut pt2).unwrap();
        assert_eq!(pt2, b"pong");
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let (mut initiator_session, mut responder_session) = completed_sessions();

        let mut ct = Vec::new();
        initiator_session
            .encrypt(b"important data", &mut ct)
            .unwrap();

        // Flip a bit in the ciphertext body (past the 8-byte nonce prefix)
        ct[12] ^= 0xFF;

        let mut pt = Vec::new();
        let result = responder_session.decrypt(&ct, &mut pt);
        assert!(result.is_err(), "tampered ciphertext should be rejected");
    }

    // ── ReplayWindow ─────────────────────────────────────────────────────────

    #[test]
    fn replay_window_accepts_sequential_nonces() {
        let mut w = ReplayWindow::new();
        for i in 0..100 {
            assert!(w.check(i), "nonce {i} should be accepted");
            w.mark(i);
        }
    }

    #[test]
    fn replay_window_rejects_duplicate() {
        let mut w = ReplayWindow::new();
        w.mark(5);
        assert!(!w.check(5), "duplicate nonce 5 should be rejected");
    }

    #[test]
    fn replay_window_rejects_too_old() {
        let mut w = ReplayWindow::new();
        w.mark(WINDOW_SIZE + 100);
        assert!(!w.check(0), "nonce 0 should be too old");
    }

    #[test]
    fn replay_window_accepts_within_window() {
        let mut w = ReplayWindow::new();
        w.mark(100);
        // Nonce 50 is within the window (100 - 50 = 50 < 2048)
        assert!(w.check(50), "nonce 50 should be within window");
    }

    #[test]
    fn replay_window_advancement() {
        let mut w = ReplayWindow::new();
        // Mark nonces 0..10
        for i in 0..10 {
            w.mark(i);
        }
        // Jump far ahead
        w.mark(5000);
        // Old nonces should now be too old
        assert!(!w.check(0));
        // Recent nonce just behind highest is fine
        assert!(w.check(4999));
    }

    // ── Explicit nonce transport ─────────────────────────────────────────────

    #[test]
    fn nonce_prefix_roundtrip() {
        let (mut i_sess, mut r_sess) = completed_sessions();

        let plaintext = b"explicit nonce test";
        let mut ct = Vec::new();
        let mut pt = Vec::new();

        i_sess.encrypt(plaintext, &mut ct).unwrap();

        // Ciphertext should be 8 (nonce) + plaintext.len() + 16 (MAC)
        assert_eq!(ct.len(), 8 + plaintext.len() + 16);

        // First 8 bytes are the LE nonce (should be 0 for first message)
        let nonce = u64::from_le_bytes(ct[..8].try_into().unwrap());
        assert_eq!(nonce, 0);

        r_sess.decrypt(&ct, &mut pt).unwrap();
        assert_eq!(pt.as_slice(), plaintext.as_slice());
    }

    #[test]
    fn out_of_order_decrypt() {
        let (mut i_sess, mut r_sess) = completed_sessions();

        // Encrypt three messages
        let mut ct0 = Vec::new();
        let mut ct1 = Vec::new();
        let mut ct2 = Vec::new();
        i_sess.encrypt(b"msg0", &mut ct0).unwrap();
        i_sess.encrypt(b"msg1", &mut ct1).unwrap();
        i_sess.encrypt(b"msg2", &mut ct2).unwrap();

        // Decrypt in order 2, 0, 1
        let mut pt = Vec::new();
        r_sess.decrypt(&ct2, &mut pt).unwrap();
        assert_eq!(pt, b"msg2");

        r_sess.decrypt(&ct0, &mut pt).unwrap();
        assert_eq!(pt, b"msg0");

        r_sess.decrypt(&ct1, &mut pt).unwrap();
        assert_eq!(pt, b"msg1");
    }

    #[test]
    fn replay_rejection_in_session() {
        let (mut i_sess, mut r_sess) = completed_sessions();

        let mut ct = Vec::new();
        let mut pt = Vec::new();
        i_sess.encrypt(b"once only", &mut ct).unwrap();

        // First decrypt succeeds
        r_sess.decrypt(&ct, &mut pt).unwrap();
        assert_eq!(pt, b"once only");

        // Second decrypt of same ciphertext is rejected
        let result = r_sess.decrypt(&ct, &mut pt);
        assert!(result.is_err(), "replayed ciphertext should be rejected");
    }

    #[test]
    fn too_short_ciphertext_rejected() {
        let (_, mut r_sess) = completed_sessions();
        let mut pt = Vec::new();
        // Less than 24 bytes (8 nonce + 16 MAC minimum)
        let result = r_sess.decrypt(&[0u8; 20], &mut pt);
        assert!(result.is_err());
    }
}
