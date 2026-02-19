//! QoS — token bucket rate limiting per session contract.
//!
//! Refill rates:
//!   Realtime   — unlimited (never throttled)
//!   Bulk       — 64 tokens/sec  (high throughput, but bounded)
//!   Background — 8 tokens/sec   (only when nothing else is active)
//!
//! Each chunk costs 1 token. Empty bucket = drop.

use std::time::Instant;
use summit_core::wire::Contract;

const BULK_RATE: f64 = 64.0;
const BULK_BURST: f64 = 32.0;
const BG_RATE: f64 = 8.0;
const BG_BURST: f64 = 4.0;

#[derive(Debug)]
pub struct TokenBucket {
    tokens: f64,
    capacity: f64,
    refill_rate: f64,
    last_refill: Instant,
    contract: Contract,
}

impl TokenBucket {
    pub fn new(contract: Contract) -> Self {
        let (capacity, refill_rate) = match contract {
            Contract::Realtime => (f64::INFINITY, f64::INFINITY),
            Contract::Bulk => (BULK_BURST, BULK_RATE),
            Contract::Background => (BG_BURST, BG_RATE),
        };
        Self {
            tokens: capacity,
            capacity,
            refill_rate,
            last_refill: Instant::now(),
            contract,
        }
    }

    /// Returns true if the chunk should be sent, false if dropped.
    pub fn allow(&mut self) -> bool {
        if matches!(self.contract, Contract::Realtime) {
            return true;
        }

        // Refill based on elapsed time
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity);
        self.last_refill = now;

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    pub fn contract(&self) -> Contract {
        self.contract
    }

    pub fn tokens(&self) -> f64 {
        self.tokens.min(self.capacity)
    }
}
