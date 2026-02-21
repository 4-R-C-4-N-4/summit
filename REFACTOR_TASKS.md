# Summit Protocol Refactoring Task List for Claude Code

## Context
Summit Protocol is a P2P mesh networking daemon. The current architecture has summitd doing too much - it handles protocol, business logic, storage, and HTTP API all in one crate. We need to separate concerns for maintainability and reusability.

## Current Structure
```
crates/
├── summit-core/        # Protocol primitives (crypto, wire format, schemas, message types)
├── summitd/            # BLOATED: daemon + services + API + storage
│   ├── main.rs         # Session management, chunk routing, discovery
│   ├── status.rs       # HTTP API endpoints (14 routes)
│   ├── message_store.rs # Message storage (DashMap)
│   ├── trust.rs        # Trust registry
│   ├── transfer.rs     # File reassembly
│   ├── cache.rs        # Chunk cache
│   └── ... (more)
├── summit-ctl/         # CLI tool
└── (astral/)           # Electron UI (separate, already good)
```

## Target Structure
```
crates/
├── summit-core/        # Protocol only (no changes needed)
├── summit-services/    # NEW: Business logic & storage
│   ├── message_store.rs
│   ├── trust.rs
│   ├── file_transfer.rs
│   ├── chunk_cache.rs
│   └── lib.rs
├── summit-api/         # NEW: HTTP server (optional feature)
│   ├── handlers.rs
│   └── lib.rs
├── summitd/            # MINIMAL: Just orchestration
│   └── main.rs
└── summit-ctl/         # No changes
```

---

## Phase 1: Create summit-services Crate

### Task 1.1: Create new crate
```bash
cargo new --lib crates/summit-services
```

**Update `Cargo.toml`:**
```toml
[workspace]
members = [
    "crates/summit-core",
    "crates/summit-services",  # ADD
    "crates/summitd",
    "crates/summit-ctl",
]

[dependencies]
dashmap = "6.1"
tokio = { version = "1", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tracing = "0.1"
summit-core = { path = "../summit-core" }
```

### Task 1.2: Move message_store.rs
**Action:**
1. Copy `crates/summitd/src/message_store.rs` → `crates/summit-services/src/message_store.rs`
2. Update imports to use `summit_core::message::MessageChunk`
3. Make all items `pub` (pub struct, pub impl)
4. Export in `crates/summit-services/src/lib.rs`:
   ```rust
   pub mod message_store;
   pub use message_store::MessageStore;
   ```

**Verify:** `cargo build -p summit-services`

### Task 1.3: Move trust.rs
**Action:**
1. Copy `crates/summitd/src/trust.rs` → `crates/summit-services/src/trust.rs`
2. Update imports
3. Make public: `TrustRegistry`, `TrustLevel`, `UntrustedBuffer`
4. Export in lib.rs

**Verify:** `cargo build -p summit-services`

### Task 1.4: Move cache.rs
**Action:**
1. Copy `crates/summitd/src/cache.rs` → `crates/summit-services/src/cache.rs`
2. Update to `pub struct ChunkCache`
3. Export in lib.rs

**Verify:** `cargo build -p summit-services`

### Task 1.5: Move transfer.rs (file reassembly)
**Action:**
1. Copy `crates/summitd/src/transfer.rs` → `crates/summit-services/src/file_transfer.rs`
2. Rename module references
3. Make `FileReassembler`, `FileMetadata` public
4. Export in lib.rs

**Verify:** `cargo build -p summit-services`

---

## Phase 2: Create summit-api Crate

### Task 2.1: Create new crate
```bash
cargo new --lib crates/summit-api
```

**Update workspace Cargo.toml:**
```toml
members = [
    "crates/summit-core",
    "crates/summit-services",
    "crates/summit-api",  # ADD
    "crates/summitd",
    "crates/summit-ctl",
]
```

**summit-api Cargo.toml:**
```toml
[dependencies]
axum = "0.8"
tokio = { version = "1", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tower-http = { version = "0.6", features = ["cors"] }
bytes = "1.8"
hex = "0.4"
tracing = "0.1"
summit-core = { path = "../summit-core" }
summit-services = { path = "../summit-services" }
```

### Task 2.2: Move status.rs → handlers.rs
**Action:**
1. Copy `crates/summitd/src/status.rs` → `crates/summit-api/src/handlers.rs`
2. Remove `#[cfg(feature = "embed-ui")]` stuff (no UI in API crate)
3. Change `StatusState` to accept services via dependency injection:
   ```rust
   pub struct ApiState {
       pub sessions: Arc<DashMap<SocketAddr, SessionInfo>>,
       pub message_store: MessageStore,
       pub trust_registry: TrustRegistry,
       pub cache: ChunkCache,
       pub file_reassembler: FileReassembler,
       pub chunk_tx: tokio::sync::mpsc::UnboundedSender<...>,
       pub keypair: Arc<Keypair>,
   }
   ```

### Task 2.3: Create API server builder
**In `crates/summit-api/src/lib.rs`:**
```rust
pub mod handlers;

use axum::Router;
use std::net::SocketAddr;

pub async fn create_api_server(
    state: handlers::ApiState,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new()
        .route("/api/status", axum::routing::get(handlers::handle_status))
        .route("/api/peers", axum::routing::get(handlers::handle_peers))
        // ... all other routes
        .with_state(state);
    
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

**Verify:** `cargo build -p summit-api`

---

## Phase 3: Refactor summitd to Use Services

### Task 3.1: Update summitd dependencies
**In `crates/summitd/Cargo.toml`:**
```toml
[dependencies]
summit-core = { path = "../summit-core" }
summit-services = { path = "../summit-services" }
summit-api = { path = "../summit-api" }
# ... keep other deps (tokio, tracing, etc.)
```

### Task 3.2: Update imports in main.rs
**Replace:**
```rust
mod message_store;
mod trust;
mod cache;
mod transfer;
mod status;

use message_store::MessageStore;
use trust::{TrustRegistry, UntrustedBuffer};
use cache::ChunkCache;
use transfer::FileReassembler;
```

**With:**
```rust
use summit_services::{MessageStore, TrustRegistry, UntrustedBuffer, ChunkCache, FileReassembler};
```

### Task 3.3: Remove moved files from summitd
**Delete:**
- `crates/summitd/src/message_store.rs`
- `crates/summitd/src/trust.rs`
- `crates/summitd/src/cache.rs`
- `crates/summitd/src/transfer.rs`
- `crates/summitd/src/status.rs`

### Task 3.4: Start API server in main.rs
**Replace the old axum setup with:**
```rust
// After creating all services (message_store, trust_registry, etc.)
let api_state = summit_api::handlers::ApiState {
    sessions: sessions.clone(),
    message_store: message_store.clone(),
    trust_registry: trust_registry.clone(),
    cache: cache.clone(),
    file_reassembler: reassembler.clone(),
    chunk_tx: chunk_tx.clone(),
    keypair: keypair.clone(),
};

tokio::spawn(async move {
    if let Err(e) = summit_api::create_api_server(api_state, 9001).await {
        tracing::error!(error = %e, "API server failed");
    }
});
```

**Verify:** `cargo build -p summitd`

---

## Phase 4: Update summit-ctl

### Task 4.1: No code changes needed
**Just update Cargo.toml if needed:**
```toml
[dependencies]
# summit-ctl only talks to HTTP API, no direct dependency on services
reqwest = { version = "0.12", features = ["json"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

**Verify:** `cargo build -p summit-ctl`

---

## Phase 5: Testing & Verification

### Task 5.1: Build all crates
```bash
cargo build --workspace --release
```

### Task 5.2: Run tests
```bash
cargo test --workspace
```

### Task 5.3: Integration test
```bash
# Terminal 1
sudo ./target/release/summitd wlp5s0

# Terminal 2
./target/release/summit-ctl status
./target/release/summit-ctl peers

# Terminal 3 (if you have two machines)
# Test file transfer and messaging
```

### Task 5.4: Update documentation
**Files to update:**
- `README.md` - Update architecture diagram
- `docs/ARCHITECTURE.md` - Create if doesn't exist, document new structure
- Each crate's README - Add purpose and API docs

---

## Phase 6: Cleanup & Polish

### Task 6.1: Remove dead code
**Search for:**
- Unused imports
- Commented-out code
- TODO comments that are resolved

### Task 6.2: Update CI/CD
**In `.github/workflows/ci.yml`:**
```yaml
- name: Build all crates
  run: |
    cargo build -p summit-core
    cargo build -p summit-services
    cargo build -p summit-api
    cargo build -p summitd
    cargo build -p summit-ctl
```

### Task 6.3: Version bump
**Update all Cargo.toml files:**
```toml
version = "0.2.0"  # Breaking change due to refactor
```

---

## Success Criteria

✅ `cargo build --workspace --release` succeeds
✅ `cargo test --workspace` passes
✅ summitd runs and serves HTTP API on :9001
✅ summit-ctl commands work
✅ File transfer works between two nodes
✅ Messaging works between trusted peers
✅ No compilation warnings
✅ Clippy passes: `cargo clippy --workspace`
✅ All crates have proper READMEs

---

## Risks & Mitigations

**Risk 1: Breaking changes during migration**
- Mitigation: Do one module at a time, verify compilation after each

**Risk 2: Circular dependencies**
- Mitigation: summit-core → summit-services → summit-api → summitd (one direction only)

**Risk 3: Shared state management**
- Mitigation: Use Arc for all shared state, pass clones explicitly

**Risk 4: HTTP API state complexity**
- Mitigation: ApiState is just a collection of Arc<Service>, no complex ownership

---

## Expected Outcome

**Clean architecture:**
```
summit-core (protocol)
    ↓
summit-services (business logic, reusable)
    ↓
summit-api (HTTP server, optional)
    ↓
summitd (thin orchestrator)
```

**Benefits:**
- Can embed services in other apps (no HTTP overhead)
- Can run headless without API
- Services are unit-testable independently
- Clear separation of concerns
- Easier to add new transports (gRPC, WebSocket, etc.)

---

## Notes for Claude Code

- Work incrementally - one phase at a time
- Run `cargo build` after each task to catch errors early
- Keep the existing functionality working throughout (no breaking the build)
- Update imports automatically when moving files
- Watch for `pub` visibility - services need public APIs
- Don't remove code from summitd until new crates compile
- Test after each phase, not just at the end

Good luck! 🚀
