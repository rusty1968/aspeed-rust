# Digest Server - Design Document (Multi-Session Edition)

**Status**: ✅ **UPDATED FOR MULTI-SESSION SUPPORT**
**Last Updated**: 2025-09-30

## Executive Summary

The **Digest Server** is a hardware-accelerated cryptographic hashing service for the Hubris operating system. It provides **multi-session** SHA-2 digest operations (SHA-256, SHA-384, SHA-512) through a type-safe IPC interface, leveraging the ASPEED HACE hardware accelerator with software-based context switching to support up to 8 concurrent hash sessions.

**Key Enhancement**: The server now supports **multiple concurrent sessions** through the new `SessionManager` API, enabling true multi-session hash operations required by modern security protocols (TLS, SSH, SPDM).

## System Architecture

### 1. Component Overview (Updated)

```
┌─────────────────────────────────────────────────────────────┐
│                      Client Tasks                            │
│    (SPDM Responder, Attestation, TLS, SSH, etc.)           │
│    - Multiple concurrent clients supported                   │
└────────────────────┬────────────────────────────────────────┘
                     │ IPC (Idol)
                     │ Multiple concurrent sessions
                     │
┌────────────────────▼────────────────────────────────────────┐
│                  Digest Server (Task)                        │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  DigestServerImpl                                     │  │
│  │  - Multi-session management (up to 8 concurrent)     │  │
│  │  - SessionManager<8> wrapper                         │  │
│  │  - Session ID validation                             │  │
│  │  - IPC request routing                               │  │
│  └──────────────┬───────────────────────────────────────┘  │
│                 │                                            │
│  ┌──────────────▼───────────────────────────────────────┐  │
│  │  SessionManager<N> (aspeed-ddk)                      │  │
│  │  - Automatic session tracking                        │  │
│  │  - Transparent context switching                     │  │
│  │  - Type-safe algorithm verification                  │  │
│  │  - Resource cleanup (finalize/cancel)                │  │
│  └──────────────┬───────────────────────────────────────┘  │
│                 │                                            │
│  ┌──────────────▼───────────────────────────────────────┐  │
│  │  OwnedDigestContext<T, MultiContextProvider>         │  │
│  │  - Move-based digest API                             │  │
│  │  - No lifetime constraints (IPC-friendly)            │  │
│  │  - Wraps HaceController                              │  │
│  └──────────────┬───────────────────────────────────────┘  │
│                 │                                            │
│  ┌──────────────▼───────────────────────────────────────┐  │
│  │  HaceController<MultiContextProvider>                │  │
│  │  - Generic over context provider                     │  │
│  │  - Hardware abstraction                              │  │
│  └──────────────┬───────────────────────────────────────┘  │
└─────────────────┼────────────────────────────────────────────┘
                  │
     ┌────────────┴────────────┐
     │                         │
┌────▼──────────────┐   ┌──────▼─────────────────┐
│MultiContextProvider│   │SingleContextProvider   │
│ (Multi-session)    │   │ (Single session)       │
│ - 4-8 contexts     │   │ - Zero-cost default    │
│ - Context switching│   │ - Single context only  │
│ - ~732 bytes/ctx   │   │ - No switching overhead│
└────┬───────────────┘   └────────────────────────┘
     │
     ▼
┌─────────────────────┐
│ ASPEED HACE Hardware│
│ (AST1060 SoC)       │
│ - DMA-based hashing │
│ - 100-200 MB/s      │
└─────────────────────┘
```

### 2. Key Components (Updated)

#### 2.1 DigestServerImpl

The main server implementation using `SessionManager`.

**State:**
```rust
pub struct DigestServerImpl {
    /// Session manager supporting N concurrent sessions
    manager: SessionManager<8>,

    /// Active session storage (IPC session ID → SessionDigest)
    sessions: [Option<ActiveDigest>; 8],

    /// Session metadata (optional)
    session_info: [Option<SessionInfo>; 8],
}

enum ActiveDigest {
    Sha256(SessionDigest<Sha2_256>),
    Sha384(SessionDigest<Sha2_384>),
    Sha512(SessionDigest<Sha2_512>),
}
```

**Responsibilities:**
- IPC request handling (init/update/finalize)
- Session ID mapping (IPC ID ↔ SessionDigest)
- Input validation
- Algorithm routing
- Error translation (internal → IPC errors)

**Key Change**: Now uses `SessionManager` instead of manual session management, reducing boilerplate by ~85%.

#### 2.2 SessionManager<N>

High-level session management API (from `aspeed-ddk::digest::session`).

**Type Parameters:**
- `N`: Maximum concurrent sessions (const generic, typically 4-8)

**State:**
```rust
pub struct SessionManager<N> {
    /// Underlying controller (None when session owns it)
    controller: Option<HaceController<MultiContextProvider>>,

    /// Session metadata
    sessions: [SessionSlot; N],

    /// Wrapping session ID counter
    next_id: u32,
}
```

**API:**
```rust
impl<N> SessionManager<N> {
    pub fn new(hace: Hace) -> Result<Self, SessionError>;

    pub fn init_sha256(&mut self) -> Result<SessionDigest<Sha2_256>, ...>;
    pub fn init_sha384(&mut self) -> Result<SessionDigest<Sha2_384>, ...>;
    pub fn init_sha512(&mut self) -> Result<SessionDigest<Sha2_512>, ...>;

    pub fn finalize<T>(&mut self, digest: SessionDigest<T>)
        -> Result<(T::Digest, SessionHandle<T>), ...>;

    pub fn cancel<T>(&mut self, digest: SessionDigest<T>) -> Result<(), ...>;

    pub fn active_count(&self) -> usize;
    pub fn max_sessions(&self) -> usize;
}
```

**Key Features:**
- Automatic session activation before operations
- Type-safe algorithm verification (compile-time)
- Resource cleanup (automatic on finalize/cancel)
- Transparent context switching

#### 2.3 SessionDigest<T>

Auto-managed digest context with session tracking.

**Structure:**
```rust
pub struct SessionDigest<T: DigestAlgorithm> {
    /// Wrapped owned context
    context: OwnedDigestContext<T, MultiContextProvider>,

    /// Provider session ID (for context switching)
    provider_session_id: usize,

    /// Manager session ID (for validation)
    manager_session_id: u32,

    /// Slot index (for cleanup)
    slot: usize,
}
```

**API:**
```rust
impl<T> SessionDigest<T> {
    pub fn update(self, data: &[u8]) -> Result<Self, SessionError>;
    pub fn handle(&self) -> SessionHandle<T>;
}
```

**Behavior:**
- `update()` automatically activates the correct session
- Move semantics prevent use-after-finalize
- No lifetimes (can be stored in structs)

#### 2.4 MultiContextProvider

Low-level context switching implementation.

**Structure:**
```rust
pub struct MultiContextProvider {
    /// Stored context states (one per session)
    contexts: [MaybeUninit<AspeedHashContext>; MAX_SESSIONS],

    /// Session allocation bitmap
    allocated: [bool; MAX_SESSIONS],

    /// Currently active session ID
    active_id: usize,

    /// Which context is loaded in hardware
    last_loaded: Option<usize>,

    /// Maximum sessions to support
    max_sessions: usize,
}
```

**Key Operations:**
```rust
impl MultiContextProvider {
    pub fn new(max_sessions: usize) -> Result<Self, SessionError>;

    pub fn allocate_session(&mut self) -> Result<usize, SessionError>;
    pub fn release_session(&mut self, session_id: usize);
    pub fn set_active_session(&mut self, session_id: usize);

    fn save_hw_to_slot(&mut self, slot_id: usize) -> Result<(), ...>;
    fn load_slot_to_hw(&mut self, slot_id: usize) -> Result<(), ...>;
}

impl HaceContextProvider for MultiContextProvider {
    fn ctx_mut(&mut self) -> Result<&mut AspeedHashContext, ...> {
        // Lazy context switching
        if self.last_loaded != Some(self.active_id) {
            if let Some(prev_id) = self.last_loaded {
                self.save_hw_to_slot(prev_id)?;
            }
            self.load_slot_to_hw(self.active_id)?;
            self.last_loaded = Some(self.active_id);
        }

        Ok(unsafe { &mut *shared_hash_ctx() })
    }
}
```

**Context Switching:**
- **Lazy**: Only switches when accessing different session
- **Copy size**: ~732 bytes per switch (save + restore)
- **Time**: ~20-30 µs @ 200 MHz
- **Security**: Volatile zeroing on release

#### 2.5 Hardware Constraints (Updated)

**Original Hardware:**
- ASPEED HACE: Single hardware context (no hardware context switching)
- MAX_CONCURRENT_SESSIONS = 1

**With Software Context Switching:**
- ASPEED HACE: Still single hardware context
- **Software**: Up to N concurrent sessions (typically N=4-8)
- MAX_CONCURRENT_SESSIONS = N (configurable via const generic)
- Context switching: Software-managed, transparent to API

## API Design (Updated)

### 3. IPC Interface (Idol)

The server exposes operations defined in `openprot-digest.idol`:

#### 3.1 Session-Based Operations

**Initialization:**
```rust
init_sha256() -> Result<u32, DigestError>
init_sha384() -> Result<u32, DigestError>
init_sha512() -> Result<u32, DigestError>
```
Returns a session ID on success. **Multiple sessions can now be active concurrently**.

**Update:**
```rust
update(session_id: u32, len: u32, data: &[u8]) -> Result<(), DigestError>
```
Incrementally adds data to the hash computation. **Context automatically switches to the correct session**.

**Finalization:**
```rust
finalize_sha256(session_id: u32, digest_out: &mut [u32; 8]) -> Result<(), DigestError>
finalize_sha384(session_id: u32, digest_out: &mut [u32; 12]) -> Result<(), DigestError>
finalize_sha512(session_id: u32, digest_out: &mut [u32; 16]) -> Result<(), DigestError>
```
Completes the hash and returns the digest, releasing the session.

#### 3.2 One-Shot Operations

```rust
digest_oneshot_sha256(len: u32, data: &[u8], digest_out: &mut [u32; 8])
    -> Result<(), DigestError>
digest_oneshot_sha384(len: u32, data: &[u8], digest_out: &mut [u32; 12])
    -> Result<(), DigestError>
digest_oneshot_sha512(len: u32, data: &[u8], digest_out: &mut [u32; 16])
    -> Result<(), DigestError>
```

**Behavior:** Temporarily allocates a session, performs operation, releases session.

#### 3.3 Memory Management

**Leased Memory:**
- Input data: `Leased<R, [u8]>` with 1024-byte limit
- Output digests: `Leased<W, [u32; N]>` (N = 8, 12, or 16)

**Zero-Copy Design:**
- Direct hardware DMA access where supported
- Stack-allocated 1024-byte buffer for Idol lease reads

### 4. Error Handling

```rust
pub enum DigestError {
    InvalidInputLength = 1,
    UnsupportedAlgorithm = 2,
    MemoryAllocationFailure = 3,
    InitializationError = 4,
    UpdateError = 5,
    FinalizationError = 6,
    Busy = 7,                          // Deprecated: now rarely occurs
    HardwareFailure = 8,
    InvalidOutputSize = 9,
    PermissionDenied = 10,
    NotInitialized = 11,
    InvalidSession = 12,
    TooManySessions = 13,              // NEW: Session limit reached
    ServerRestarted = 100,
}
```

**Key Change**: `TooManySessions` replaces most `Busy` errors. Server can now handle multiple concurrent requests.

## Implementation Details

### 5. Session Management (Updated)

#### 5.1 Session Lifecycle (Multi-Session)

```
┌─────────────┐
│   Client A  │
└──────┬──────┘
       │ init_sha256()
       ▼
┌─────────────────────────────────────┐
│ DigestServer::handle_init_sha256()  │
│ - Find free IPC session slot        │
│ - manager.init_sha256()              │
│   → Allocates provider session      │
│   → Sets active session              │
│   → Returns SessionDigest            │
│ - Store in sessions[slot]            │
│ - Return IPC session ID              │
└──────┬──────────────────────────────┘
       │
       │ Client B: init_sha384() (concurrent!)
       │
┌──────▼──────────────────────────────┐
│ Different session slot allocated     │
│ Both sessions can now be active      │
└──────┬──────────────────────────────┘
       │
       │ Client A: update(session_A, data)
       ▼
┌─────────────────────────────────────┐
│ SessionDigest::update()              │
│ 1. Activate session A                │
│    - provider.set_active_session()   │
│ 2. Context switch (if needed)        │
│    - Save session B context          │
│    - Load session A context          │
│ 3. Perform update                    │
└──────┬──────────────────────────────┘
       │
       │ Client B: update(session_B, data) (interleaved!)
       ▼
┌─────────────────────────────────────┐
│ Context automatically switches       │
│ to session B                         │
└──────┬──────────────────────────────┘
       │
       │ Client A: finalize(session_A)
       ▼
┌─────────────────────────────────────┐
│ manager.finalize(digest_A)           │
│ - Activate session A                 │
│ - Finalize digest                    │
│ - Release provider session           │
│ - Return controller                  │
│ - Free IPC session slot              │
└─────────────────────────────────────┘
```

**Key Difference**: Multiple sessions can coexist, with automatic context switching.

#### 5.2 Simplified Server Code

**Before (Manual Management - ~80 lines)**:
```rust
pub fn handle_init_sha256(&mut self) -> Result<u32, DigestError> {
    let slot = self.find_free_slot()?;
    let mut controller = self.controller.take()?;
    let session_id = controller.provider_mut().allocate_session()?;
    controller.provider_mut().set_active_session(session_id);
    let ctx = controller.init(Sha2_256::default())?;
    self.sessions[slot] = Some(SessionState::Sha256 { session_id, ctx });
    Ok(slot as u32)
    // ... 70 more lines of boilerplate ...
}
```

**After (SessionManager - ~12 lines)**:
```rust
pub fn handle_init_sha256(&mut self) -> Result<u32, DigestError> {
    let digest = self.manager.init_sha256()
        .map_err(|_| DigestError::TooManySessions)?;

    let slot = self.find_free_slot()?;
    self.sessions[slot] = Some(ActiveDigest::Sha256(digest));

    Ok(slot as u32)
}
```

**Code Reduction**: ~85% fewer lines, automatic session management.

### 6. Hardware Abstraction (Updated)

#### 6.1 HAL Traits (openprot-hal-blocking)

**DigestInit Trait (Owned API):**
```rust
pub mod owned {
    pub trait DigestInit<A: DigestAlgorithm> {
        type Context: DigestOp;
        type Output;

        fn init(self, params: A) -> Result<Self::Context, Self::Error>;
    }

    pub trait DigestOp {
        type Output;
        type Controller;

        fn update(self, data: &[u8]) -> Result<Self, Self::Error>;
        fn finalize(self) -> Result<(Self::Output, Self::Controller), Self::Error>;
        fn cancel(self) -> Self::Controller;
    }
}
```

**Implementations:**
```rust
impl<P> DigestInit<Sha2_256> for HaceController<P> {
    type Context = OwnedDigestContext<Sha2_256, P>;
    type Output = Digest<8>;

    fn init(self, _: Sha2_256) -> Result<Self::Context, ...> { ... }
}

impl<P> DigestOp for OwnedDigestContext<Sha2_256, P> {
    type Output = Digest<8>;
    type Controller = HaceController<P>;

    fn update(self, data: &[u8]) -> Result<Self, ...> { ... }
    fn finalize(self) -> Result<(Digest<8>, HaceController<P>), ...> { ... }
    fn cancel(self) -> HaceController<P> { ... }
}
```

**Key Feature**: Move semantics enable IPC persistence (no lifetimes).

#### 6.2 Hardware Capabilities (Updated)

```rust
pub trait DigestHardwareCapabilities {
    const MAX_CONCURRENT_SESSIONS: usize;
    const SUPPORTS_HARDWARE_CONTEXT_SWITCHING: bool;
}

// Original (single session)
impl DigestHardwareCapabilities for HaceController<SingleContextProvider> {
    const MAX_CONCURRENT_SESSIONS: usize = 1;
    const SUPPORTS_HARDWARE_CONTEXT_SWITCHING: bool = false;
}

// NEW: Multi-session with software context switching
impl DigestHardwareCapabilities for HaceController<MultiContextProvider> {
    const MAX_CONCURRENT_SESSIONS: usize = 4; // Configurable up to 8
    const SUPPORTS_HARDWARE_CONTEXT_SWITCHING: bool = false; // Still no HW support
}
```

**Note**: Hardware still doesn't support context switching, but software emulates it transparently.

### 7. Type Safety Guarantees (Enhanced)

#### 7.1 Compile-Time Algorithm Matching

The SessionManager enforces algorithm correctness at compile time:

```rust
// ✅ Compiles: Correct algorithm
let session = manager.init_sha256()?;
let (digest, _) = manager.finalize(session)?;

// ❌ Compile error: Type mismatch
let session = manager.init_sha256()?;
manager.finalize_sha384(session)?;  // ERROR: expected SessionDigest<Sha2_256>, found SessionDigest<Sha2_384>
```

**Benefit**: Impossible to finalize a session with the wrong algorithm handler.

#### 7.2 Session ID Validation

Runtime validation prevents session confusion:

```rust
impl SessionManager<N> {
    pub fn finalize<T>(&mut self, digest: SessionDigest<T>) -> Result<...> {
        // Validate slot
        let slot_data = self.sessions.get(digest.slot)?;

        // Validate session ID (prevents stale handles)
        if slot_data.session_id != digest.manager_session_id {
            return Err(SessionError::InvalidSession);
        }

        // ... proceed with finalization ...
    }
}
```

### 8. Initialization Sequence (Updated)

```rust
#[no_mangle]
pub extern "C" fn main() -> ! {
    // 1. Initialize hardware peripherals (ASPEED platform)
    let peripherals = unsafe { Peripherals::steal() };
    let mut syscon = SysCon::new(DummyDelay, peripherals.scu);

    // Enable HACE clock
    syscon.enable(&ClockId::ClkYCLK);

    // Release HACE from reset
    syscon.reset_deassert(&ResetId::RstHACE);

    // 2. Create SessionManager with multi-context support
    let manager = SessionManager::<8>::new(peripherals.hace)
        .expect("Failed to initialize SessionManager");

    // 3. Initialize server with manager
    let mut server = DigestServerImpl::new(manager);

    // 4. Enter IPC loop
    let mut incoming = [0u8; idl::INCOMING_SIZE];
    loop {
        idol_runtime::dispatch(&mut incoming, &mut server);
    }
}
```

**Key Change**: Uses `SessionManager::new()` instead of direct `HaceController::new()`.

## Performance Characteristics (Updated)

### 13. Benchmarks & Estimates

**IPC Overhead:**
- Session init: ~2-5 µs (context switch + HAL init)
- Update: ~1-3 µs + DMA transfer time
- Finalize: ~2-5 µs + final block processing

**Context Switch Overhead (NEW):**
- Save context: ~10-15 µs (732 byte copy)
- Load context: ~10-15 µs (732 byte copy)
- Total switch: ~20-30 µs (only when switching sessions)
- Lazy optimization: Zero cost when staying in same session

**Hardware Performance (ASPEED HACE):**
- SHA-256: ~100-200 MB/s (hardware-dependent)
- SHA-384/512: ~80-150 MB/s

**Memory Footprint (Updated):**
- Server state: ~3.2 KB (SessionManager<8> with MultiContextProvider)
  - Base: ~16 bytes (controller pointer)
  - Provider: ~3 KB (4 contexts × 732 bytes + metadata)
  - Sessions: ~128 bytes (8 × 16 bytes)
- Stack usage per operation: ~1.5 KB (1024-byte buffer + overhead)

**Throughput Analysis:**
For typical use case (1 KB hash operations):
- Hash time: ~5-10 µs (hardware)
- Context switch: ~20-30 µs (amortized across operations)
- Effective overhead: <10% for interleaved multi-session operations

## Design Decisions & Rationale (Updated)

### 9. Key Design Choices

#### 9.1 Multi-Session via Software Context Switching

**Decision:** Support multiple concurrent sessions through software context switching.

**Rationale:**
- Hardware constraint: HACE has no native context switching
- Security protocols (TLS, SSH, SPDM) require concurrent hash operations
- Context size (~732 bytes) is manageable for software switching
- Switch time (~20-30 µs) is acceptable compared to hash time
- Lazy switching minimizes overhead when staying in one session

**Alternative Considered:** Blocking single-session model
- **Rejected:** Unacceptable for modern security protocols requiring concurrent operations

#### 9.2 SessionManager API

**Decision:** Create dedicated high-level `SessionManager` API.

**Rationale:**
- Reduces IPC server code by ~85%
- Automatic session tracking eliminates boilerplate
- Type-safe algorithm verification (compile-time)
- Resource cleanup (automatic via finalize/cancel)
- Clean separation of concerns

**Alternative Considered:** Manual provider management
- **Rejected:** Too much boilerplate, error-prone, hard to maintain

#### 9.3 Owned API Pattern (Retained)

**Decision:** Continue using owned contexts with move semantics.

**Rationale:**
- Prevents context reuse after finalization
- No lifetimes = IPC-friendly (can store in structs)
- Compiler-enforced state machine (init → update* → finalize)
- Hardware controller automatically returned on finalize

**Alternative Considered:** Scoped (borrowed) API
- **Rejected:** Cannot cross IPC boundaries due to lifetimes, though has better affinity with context switching

#### 9.4 Const Generic `N` for Session Count

**Decision:** Use const generic parameter for maximum sessions.

**Rationale:**
- Compile-time validation of session counts
- Zero runtime overhead
- Type-level documentation (e.g., `SessionManager<8>`)
- Enables future optimizations

**Trade-off**: Cannot change session count at runtime (not needed for IPC servers)

## Comparison: Old vs New Architecture

| Aspect | Old (Single Session) | New (Multi-Session) |
|--------|---------------------|---------------------|
| **Max concurrent sessions** | 1 | 4-8 (configurable) |
| **Session blocking** | Yes (busy error) | No (up to N sessions) |
| **Context switching** | N/A | Software (~20-30 µs) |
| **Memory overhead** | ~732 bytes | ~3 KB (for N=4) |
| **Server code complexity** | ~80 lines/handler | ~12 lines/handler |
| **Type safety** | Runtime (enum checks) | Compile-time (generics) |
| **Resource cleanup** | Manual | Automatic |
| **Error handling** | Complex (manual cleanup) | Simple (auto cleanup) |
| **Use case fit** | Single-client scenarios | Multi-client protocols |

## Usage Examples (Updated)

### 10. Client Patterns

#### 10.1 Concurrent Multi-Client Hashing

```rust
// Client A: Hash certificate
let session_a = digest.init_sha256()?;
digest.update(session_a, cert_data_chunk1)?;

// Client B: Hash handshake (concurrent with A!)
let session_b = digest.init_sha384()?;
digest.update(session_b, handshake_chunk1)?;

// Client C: Hash attestation (concurrent with A and B!)
let session_c = digest.init_sha512()?;
digest.update(session_c, attestation_data)?;

// Continue interleaving updates...
digest.update(session_a, cert_data_chunk2)?;  // Context switches to A
digest.update(session_b, handshake_chunk2)?;   // Context switches to B

// Finalize in any order
let mut hash_c = [0u32; 16];
digest.finalize_sha512(session_c, &mut hash_c)?;

let mut hash_a = [0u32; 8];
digest.finalize_sha256(session_a, &mut hash_a)?;

let mut hash_b = [0u32; 12];
digest.finalize_sha384(session_b, &mut hash_b)?;
```

**Key Feature**: All three sessions can be active simultaneously with automatic context switching.

#### 10.2 Session-Based Hashing (Streaming)

```rust
// Initialize session
let session_id = digest.init_sha256()?;

// Stream data in chunks (session stays active)
let chunk1 = b"Hello, ";
let chunk2 = b"World!";
digest.update(session_id, chunk1.len() as u32, chunk1)?;
digest.update(session_id, chunk2.len() as u32, chunk2)?;

// Finalize and get result
let mut hash = [0u32; 8];
digest.finalize_sha256(session_id, &mut hash)?;
```

**No Change**: Single-session usage works exactly as before.

## Migration Guide

### For Existing Servers

#### Option 1: Minimal Change (Stay Single-Session)

Keep using `SingleContextProvider` (zero changes required):

```rust
let controller = HaceController::new(hace);  // Still works!
```

#### Option 2: Enable Multi-Session (Recommended)

Adopt `SessionManager`:

```rust
// Old:
let controller = HaceController::new(hace);
let mut server = DigestServerImpl::new(controller);

// New:
let manager = SessionManager::<8>::new(hace)?;
let mut server = DigestServerImpl::new(manager);

// IPC handlers become simpler (~85% code reduction)
```

## References

- **Architecture**: [digest-server-architecture.md](docs/digest-server-architecture.md)
- **Multi-Context Design**: [hace-multi-context-design.md](docs/hace-multi-context-design.md)
- **IPC Integration**: [multi-context-ipc-integration.md](docs/multi-context-ipc-integration.md)
- **SessionManager API**: [multi-session-api-design.md](docs/multi-session-api-design.md)
- **Implementation**: [src/digest/session.rs](src/digest/session.rs)

---

**Document Version:** 2.0 (Multi-Session Edition)
**Last Updated:** 2025-09-30
**Author:** Reverse-engineered from source code, updated with SessionManager integration
**Status:** Production-ready with multi-session support (SHA-2 family)
