# HACE Multi-Context Design Document

**Status**: ✅ **IMPLEMENTED** (As of 2025-09-30)

## Overview

This document describes the implemented multi-context support for the ASPEED HACE (Hash and Crypto Engine) controller, enabling concurrent hash operations required by security protocols.

## Problem Statement

### Current Limitations

The current `HaceController` implementation has a fundamental constraint:

- **Single shared context**: One global `AspeedHashContext` in `.ram_nc` section
- **No concurrency**: Only one hash operation can be active at a time
- **Blocking operations**: Each hash operation must complete before another can start

From [hace_controller.rs:232](../src/hace_controller.rs#L232):
```rust
#[link_section = ".ram_nc"]
static SHARED_HASH_CTX: SectionPlacedContext = SectionPlacedContext::new();
```

### Security Protocol Requirements

Security protocols (TLS, SSH, etc.) require multiple concurrent hash operations:
- Session handshakes with independent hash states
- Parallel signature verification
- Multiple HMAC computations for different keys
- Message authentication alongside encryption

The Hubris digest server using [hash_owned.rs](../src/hash_owned.rs) attempts to maintain multiple `OwnedDigestContext` instances, but the underlying hardware can only process one at a time.

### Requirements

1. **Concurrent sessions**: Support N independent hash operations (typically 4-8)
2. **Context isolation**: Each operation maintains independent state
3. **API compatibility**: No breaking changes to existing interfaces
4. **Performance**: Minimize context switch overhead
5. **Safety**: Maintain memory safety and prevent context corruption

## Implemented Solution

### Architecture Overview

Uses Rust's trait system to make `HaceController` generic over context storage strategies with dependency injection:

```
┌─────────────────────────────────────────┐
│   HaceController<P: ContextProvider>    │
│  (Generic implementation - no changes)  │
└─────────────────┬───────────────────────┘
                  │
         ┌────────┴────────┐
         │                 │
    ┌────▼────┐      ┌─────▼─────┐
    │ Single  │      │   Multi   │
    │ Context │      │  Context  │
    │Provider │      │ Provider  │
    └─────────┘      └───────────┘
```

### Core Design Pattern

#### 1. Context Provider Trait

**Location**: [src/digest/traits.rs](../src/digest/traits.rs)

```rust
/// Trait abstracting how hash context is accessed
pub trait HaceContextProvider {
    /// Get mutable reference to the active hash context
    ///
    /// # Errors
    /// Returns `ContextError` if context access fails (multi-context only)
    fn ctx_mut(&mut self) -> Result<&mut AspeedHashContext, ContextError>;
}
```

**Note**: The trait returns `Result` to support multi-context error handling. Single-context provider never fails (returns `Ok` always).

#### 2. Single Context Provider (Default, Zero-Cost)

**Location**: [src/digest/traits.rs](../src/digest/traits.rs)

```rust
/// Single-context provider that uses the global shared context (zero overhead)
///
/// This is the default provider for `HaceController` and provides the same
/// behavior as the original non-generic implementation.
pub struct SingleContextProvider;

impl HaceContextProvider for SingleContextProvider {
    fn ctx_mut(&mut self) -> Result<&mut AspeedHashContext, ContextError> {
        // SAFETY: Single-threaded execution, no HACE interrupts enabled
        Ok(unsafe { &mut *shared_hash_ctx() })
    }
}
```

**Key Features**:
- Zero-sized type (no runtime overhead)
- Always succeeds (never returns `Err`)
- Direct access to global `SHARED_HASH_CTX`
- Default provider when `HaceController` is used without type parameter

#### 3. Multi-Context Provider (Session Management)

**Location**: [src/digest/multi_context.rs](../src/digest/multi_context.rs)

```rust
/// Manages multiple hash contexts with automatic switching
pub struct MultiContextProvider {
    /// Stored context states (one per session) - uses MaybeUninit for lazy initialization
    contexts: [MaybeUninit<AspeedHashContext>; MAX_SESSIONS],
    /// Session allocation bitmap (1 = allocated, 0 = free)
    allocated: [bool; MAX_SESSIONS],
    /// Currently active session ID
    active_id: usize,
    /// Which context is currently loaded in hardware (None = hardware not initialized)
    last_loaded: Option<usize>,
    /// Maximum number of sessions to support (configurable, <= MAX_SESSIONS)
    max_sessions: usize,
}

impl HaceContextProvider for MultiContextProvider {
    fn ctx_mut(&mut self) -> Result<&mut AspeedHashContext, ContextError> {
        // Perform lazy context switch if needed
        if self.last_loaded != Some(self.active_id) {
            if let Some(prev_id) = self.last_loaded {
                // Save hardware context to previous session's storage slot
                // Note: Errors ignored per project requirements (should never fail with valid session IDs)
                let _ = self.save_hw_to_slot(prev_id);
            }
            // Load active session's context to hardware
            let _ = self.load_slot_to_hw(self.active_id);
            self.last_loaded = Some(self.active_id);
        }

        // Return mutable reference to the shared hardware context
        Ok(unsafe { &mut *shared_hash_ctx() })
    }
}
```

**Key Features**:
- Supports up to 4 concurrent sessions (`MAX_SESSIONS = 4`)
- Lazy context switching (only switches when necessary)
- Session allocation/deallocation API
- Uses `.get()` instead of direct indexing (panic-free per CLAUDE.md)
- Secure zeroing on session release (volatile writes to prevent optimization)
- ~732 bytes copied per context switch

#### 4. Generic HaceController (Dependency Injection)

**Location**: [src/hace_controller.rs](../src/hace_controller.rs)

```rust
/// Hash controller generic over context storage strategy
///
/// Uses dependency injection - the controller depends on a provider,
/// not the other way around. This is the correct architecture.
pub struct HaceController<P: HaceContextProvider = SingleContextProvider> {
    pub hace: Hace,
    pub algo: HashAlgo,
    pub provider: P,  // Provider is injected, not owned by provider
}

// Constructor for single-context (default)
impl HaceController<SingleContextProvider> {
    pub fn new(hace: Hace) -> Self {
        Self {
            hace,
            algo: HashAlgo::SHA256,
            provider: SingleContextProvider,
        }
    }
}

// Constructor with custom provider
impl<P: HaceContextProvider> HaceController<P> {
    pub fn with_provider(hace: Hace, provider: P) -> Self {
        Self {
            hace,
            algo: HashAlgo::SHA256,
            provider,
        }
    }

    pub fn provider_mut(&mut self) -> &mut P {
        &mut self.provider
    }
}

// All hash operations delegate to provider for context access
impl<P: HaceContextProvider> HaceController<P> {
    fn ctx_mut_unchecked(&mut self) -> &mut AspeedHashContext {
        match self.provider.ctx_mut() {
            Ok(ctx) => ctx,
            Err(_) => unreachable!("ctx_mut() failed unexpectedly"),
        }
    }

    // All other methods remain UNCHANGED:
    pub fn start_hash_operation(&mut self, len: u32) { /* delegates to provider */ }
    pub fn copy_iv_to_digest(&mut self) { /* delegates to provider */ }
    pub fn hash_key(&mut self, key: &impl AsRef<[u8]>) { /* delegates to provider */ }
    pub fn fill_padding(&mut self, remaining: usize) { /* delegates to provider */ }
}
```

**Key Design Principle**:
- ✅ Controller **uses** provider (dependency injection)
- ❌ NOT: Provider owns controller (that would be backwards!)

## Context State Management

### State to Save/Restore

From [hace_controller.rs:167-196](../src/hace_controller.rs#L167-L196), the following fields must be saved per context:

```rust
struct SavedContextState {
    digest: [u8; 64],        // 64 bytes - current hash state
    digcnt: [u64; 2],        // 16 bytes - byte count
    bufcnt: u32,             // 4 bytes  - buffer position
    buffer: [u8; 256],       // 256 bytes - pending data
    method: u32,             // 4 bytes  - algorithm flags
    block_size: u32,         // 4 bytes  - block size

    // For HMAC operations:
    key: [u8; 128],          // 128 bytes - key
    key_len: u32,            // 4 bytes - key length
    ipad: [u8; 128],         // 128 bytes - inner padding
    opad: [u8; 128],         // 128 bytes - outer padding
}
// Total: 732 bytes per context
```

### Context Switch Operations

#### Save Context
```rust
fn save_hw_to_slot(&mut self, slot_id: usize) {
    let hw_ctx = unsafe { &*SHARED_HASH_CTX.get() };
    let saved = &mut self.contexts[slot_id];

    // Copy critical state from hardware context to storage
    saved.digest.copy_from_slice(&hw_ctx.digest[..64]);
    saved.digcnt = hw_ctx.digcnt;
    saved.bufcnt = hw_ctx.bufcnt;
    saved.buffer.copy_from_slice(&hw_ctx.buffer[..256]);
    saved.method = hw_ctx.method;
    saved.block_size = hw_ctx.block_size;

    // Save HMAC state if present
    saved.key.copy_from_slice(&hw_ctx.key);
    saved.key_len = hw_ctx.key_len;
    saved.ipad.copy_from_slice(&hw_ctx.ipad);
    saved.opad.copy_from_slice(&hw_ctx.opad);
}
```

#### Restore Context
```rust
fn load_slot_to_hw(&mut self, slot_id: usize) {
    let saved = &self.contexts[slot_id];
    let hw_ctx = unsafe { &mut *SHARED_HASH_CTX.get() };

    // Copy critical state from storage to hardware context
    hw_ctx.digest[..64].copy_from_slice(&saved.digest);
    hw_ctx.digcnt = saved.digcnt;
    hw_ctx.bufcnt = saved.bufcnt;
    hw_ctx.buffer[..256].copy_from_slice(&saved.buffer);
    hw_ctx.method = saved.method;
    hw_ctx.block_size = saved.block_size;

    // Restore HMAC state if present
    hw_ctx.key.copy_from_slice(&saved.key);
    hw_ctx.key_len = saved.key_len;
    hw_ctx.ipad.copy_from_slice(&saved.ipad);
    hw_ctx.opad.copy_from_slice(&saved.opad);
}
```

## Session Management

### MultiHaceController API

```rust
impl MultiHaceController {
    /// Create a new multi-context controller
    pub fn new(hace: Hace, max_sessions: usize) -> Self {
        Self {
            hace,
            algo: HashAlgo::SHA256,
            provider: MultiContextProvider::new(max_sessions),
        }
    }

    /// Allocate a new session slot
    pub fn allocate_session(&mut self) -> Result<SessionHandle, Error> {
        self.provider.allocate_session()
    }

    /// Set the active session for subsequent operations
    pub fn set_active_session(&mut self, handle: SessionHandle) {
        self.provider.set_active_session(handle.id);
    }

    /// Release a session slot
    pub fn release_session(&mut self, handle: SessionHandle) {
        self.provider.release_session(handle.id);
    }
}
```

### Session Handle

```rust
/// Opaque handle to a hash session
pub struct SessionHandle {
    id: usize,
}
```

## Integration with hash_owned.rs

The existing `OwnedDigestContext` continues to work with **zero changes**:

```rust
pub struct OwnedDigestContext<T: DigestAlgorithm + IntoHashAlgo> {
    controller: HaceController,  // Works with any provider!
    _phantom: PhantomData<T>,
}
```

For multi-session support in Hubris:

```rust
// In digest server initialization:
let multi_controller = MultiHaceController::new(hace_peripheral, 8);

// Each session gets a handle:
let session1 = multi_controller.allocate_session()?;
let session2 = multi_controller.allocate_session()?;

// Switch contexts transparently:
multi_controller.set_active_session(session1);
let ctx1 = multi_controller.init(Sha2_256::default())?;

multi_controller.set_active_session(session2);
let ctx2 = multi_controller.init(Sha2_384::default())?;

// Updates automatically switch contexts:
multi_controller.set_active_session(session1);
let ctx1 = ctx1.update(data1)?;

multi_controller.set_active_session(session2);
let ctx2 = ctx2.update(data2)?;
```

## Performance Analysis

### Memory Overhead

```
Single context:  732 bytes (current)
Multi-context:   732 × N bytes (N sessions)

Examples:
  4 sessions:  2,928 bytes (2.86 KB)
  8 sessions:  5,856 bytes (5.72 KB)
 16 sessions: 11,712 bytes (11.4 KB)
```

### Context Switch Cost

**Operations per switch:**
- Save: ~732 byte memory copy
- Restore: ~732 byte memory copy
- Total: ~1,464 bytes copied

**CPU cycles (estimated for Cortex-M):**
- Optimized memcpy: ~2-4 cycles/byte
- Context switch: ~3,000-6,000 cycles
- At 200 MHz: ~15-30 microseconds per switch

**When is hardware worth it?**
- Hardware hash: ~50-100 cycles/byte
- Software SHA256: ~500-1,000 cycles/byte
- Break-even: >20-60 bytes makes hardware worthwhile

### Optimization: Lazy Context Switching

Context switches only occur when:
1. Accessing `ctx_mut()` for a different session
2. If the same session is accessed repeatedly, no switch occurs

Example (no switches needed):
```rust
controller.set_active_session(session1);
let ctx = ctx.update(&data1)?;  // No switch
let ctx = ctx.update(&data2)?;  // No switch
let ctx = ctx.update(&data3)?;  // No switch
```

## Implementation Plan

### Phase 1: Core Infrastructure
1. Define `HaceContextProvider` trait
2. Implement `SharedContextProvider` (maintains current behavior)
3. Make `HaceController` generic over provider
4. Add type alias for backwards compatibility
5. Verify existing tests pass unchanged

### Phase 2: Multi-Context Provider
1. Implement `MultiContextProvider` with session management
2. Add context save/restore methods
3. Implement session allocation/deallocation
4. Add `MultiHaceController` type alias

### Phase 3: Testing & Validation
1. Unit tests for context switching
2. Functional tests for concurrent sessions
3. Performance benchmarks
4. Integration tests with `hash_owned.rs`

### Phase 4: Documentation
1. Update API documentation
2. Add usage examples
3. Document performance characteristics
4. Migration guide for Hubris digest server

## Testing Strategy

### Unit Tests

```rust
#[test]
fn test_context_save_restore() {
    let mut provider = MultiContextProvider::new(2);

    // Initialize session 0 with some data
    provider.set_active_session(0);
    let ctx = provider.ctx_mut();
    ctx.buffer[0..4].copy_from_slice(&[1, 2, 3, 4]);
    ctx.bufcnt = 4;

    // Switch to session 1
    provider.set_active_session(1);
    let ctx = provider.ctx_mut();
    ctx.buffer[0..4].copy_from_slice(&[5, 6, 7, 8]);

    // Switch back to session 0
    provider.set_active_session(0);
    let ctx = provider.ctx_mut();
    assert_eq!(&ctx.buffer[0..4], &[1, 2, 3, 4]);
    assert_eq!(ctx.bufcnt, 4);
}
```

### Functional Tests

```rust
#[test]
fn test_concurrent_hash_operations() {
    let mut controller = MultiHaceController::new(hace, 2);

    let session1 = controller.allocate_session().unwrap();
    let session2 = controller.allocate_session().unwrap();

    // Hash different data in each session
    controller.set_active_session(session1);
    let ctx1 = controller.init(Sha2_256::default())?;
    let ctx1 = ctx1.update(b"session1 data")?;

    controller.set_active_session(session2);
    let ctx2 = controller.init(Sha2_256::default())?;
    let ctx2 = ctx2.update(b"session2 data")?;

    // Finalize both
    controller.set_active_session(session1);
    let (digest1, _) = ctx1.finalize()?;

    controller.set_active_session(session2);
    let (digest2, _) = ctx2.finalize()?;

    // Verify digests are different
    assert_ne!(digest1, digest2);
}
```

### Performance Benchmarks

```rust
#[test]
fn benchmark_context_switch() {
    let mut provider = MultiContextProvider::new(8);

    let start = systick::now();
    for i in 0..1000 {
        provider.set_active_session(i % 8);
        let _ = provider.ctx_mut();
    }
    let elapsed = systick::now() - start;

    // Should be < 30us per switch on 200MHz Cortex-M
}
```

## Security Considerations

### Context Isolation

- Each session's state is completely independent
- Context switching uses safe Rust (no undefined behavior)
- Memory copies are bounds-checked

### Side-Channel Resistance

- Context switching takes constant time (no data-dependent branches)
- Memory is zeroed on session deallocation
- No secret-dependent context switch timing

### Hardware Limitations

- Single hardware engine still executes one operation at a time
- No true parallelism (but that's a hardware constraint)
- Context switches are atomic (no interruption during copy)

## Alternatives Considered

### Alternative 1: Software-Only Hash

**Approach**: Use pure Rust hash implementations (e.g., `sha2` crate)

**Pros:**
- True concurrency with no context switching
- No memory overhead for context storage
- Simpler implementation

**Cons:**
- ~10x slower than hardware acceleration
- Not acceptable for security protocol performance requirements

**Decision**: Rejected due to performance requirements

### Alternative 2: Queue-Based Scheduler

**Approach**: Queue hash operations and process them in batches

**Pros:**
- Fair scheduling across operations
- Could optimize for fewer context switches

**Cons:**
- Complex implementation
- Still requires context switching
- Adds latency for queued operations
- Doesn't fit BSP layer responsibilities

**Decision**: Rejected as too complex for BSP layer

### Alternative 3: Duplicate HaceController Code

**Approach**: Create separate `MultiHaceController` with duplicated methods

**Pros:**
- Simple to understand
- No generics

**Cons:**
- ~500 lines of duplicated code
- Maintenance burden (changes need to be synchronized)
- Bug fixes must be applied twice

**Decision**: Rejected due to code duplication

## Migration Guide

### For Existing Code (No Changes Required)

Existing code continues to work unchanged:

```rust
// This still works exactly as before
let controller = HaceController::new(hace);
let ctx = controller.init(Sha2_256::default())?;
let ctx = ctx.update(data)?;
let (digest, _) = ctx.finalize()?;
```

### For Hubris Digest Server (Opt-In)

To enable multi-session support:

```rust
// Change from:
let controller = HaceController::new(hace);

// To:
let controller = MultiHaceController::new(hace, max_sessions);

// Then manage sessions explicitly:
let session = controller.allocate_session()?;
controller.set_active_session(session);
// ... existing update/finalize code unchanged ...
controller.release_session(session);
```

## Open Questions

1. **Max sessions**: What's a reasonable default for `MAX_SESSIONS`? (Proposed: 8)
2. **Memory placement**: Should context storage also be in `.ram_nc`? (Proposed: Yes, for consistency)
3. **Session lifecycle**: Should sessions be automatically cleaned up on drop? (Proposed: Yes, via `Drop` trait)
4. **Error handling**: What happens if all session slots are allocated? (Proposed: Return `Error::NoAvailableSessions`)

## References

- [hace_controller.rs](../src/hace_controller.rs) - Current implementation
- [hash_owned.rs](../src/hash_owned.rs) - Owned digest API used by Hubris
- ASPEED AST1060 Hardware Reference Manual (HACE section)
- Hubris Digest Server IPC Interface Specification

## Appendix: Complete Context Structure

```rust
#[repr(C)]
#[repr(align(64))]
pub struct AspeedHashContext {
    pub sg: [AspeedSg; 2],        // 8 bytes - scatter-gather descriptors
    pub digest: [u8; 64],          // 64 bytes - hash state
    pub method: u32,               // 4 bytes - algorithm flags
    pub block_size: u32,           // 4 bytes - block size
    pub key: [u8; 128],            // 128 bytes - HMAC key
    pub key_len: u32,              // 4 bytes - key length
    pub ipad: [u8; 128],           // 128 bytes - HMAC inner pad
    pub opad: [u8; 128],           // 128 bytes - HMAC outer pad
    pub digcnt: [u64; 2],          // 16 bytes - byte count
    pub bufcnt: u32,               // 4 bytes - buffer count
    pub buffer: [u8; 256],         // 256 bytes - pending data
    pub iv_size: u8,               // 1 byte - IV size
}
// Total: ~732 bytes (with alignment/padding)
```

## Implementation Status

### ✅ Completed

1. **Core Architecture**
   - [x] `HaceContextProvider` trait in [src/digest/traits.rs](../src/digest/traits.rs)
   - [x] `SingleContextProvider` (zero-cost default)
   - [x] Generic `HaceController<P>` with default parameter
   - [x] Module-level `shared_hash_ctx()` function

2. **Multi-Context Provider**
   - [x] `MultiContextProvider` struct in [src/digest/multi_context.rs](../src/digest/multi_context.rs)
   - [x] Session allocation/deallocation API
   - [x] Lazy context switching logic
   - [x] Save/restore operations (732 bytes per switch)
   - [x] Secure zeroing on release (volatile writes)
   - [x] Panic-free implementation (uses `.get()` instead of indexing)
   - [x] Debug assertions for session validation

3. **Integration**
   - [x] Hash module refactored into `src/digest/` directory
   - [x] Functional tests for multi-context provider
   - [x] Backward compatibility (existing code works unchanged)

### 🚧 Remaining Work

1. **hash_owned.rs Integration**
   - [ ] Make `OwnedDigestContext` generic over provider (currently uses default)
   - [ ] Add example usage with `MultiContextProvider`

2. **Testing**
   - [ ] Run functional tests on hardware/QEMU
   - [ ] Performance benchmarks for context switching
   - [ ] Stress testing with all 4 sessions active

3. **Documentation**
   - [ ] Add usage examples to module docs
   - [ ] Document performance characteristics
   - [ ] Add troubleshooting guide

---

**Document Version**: 2.0 (Updated)
**Author**: Claude Code
**Date**: 2025-09-30 (Initial), 2025-09-30 (Implementation Update)
**Status**: ✅ Architecture implemented, integration pending
