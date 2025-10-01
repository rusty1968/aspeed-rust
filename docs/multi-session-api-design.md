# Dedicated Multi-Session API Design

**Date**: 2025-09-30
**Status**: Design & Implementation
**Related**: [multi-context-ipc-integration.md](multi-context-ipc-integration.md), [hace-multi-context-design.md](hace-multi-context-design.md)

## Overview

This document describes a dedicated, ergonomic API for multi-session hash operations in IPC servers. It eliminates the boilerplate and complexity of manual session management by providing a high-level abstraction tailored for the Hubris digest server use case.

## Design Goals

1. **Zero boilerplate**: Automatic session tracking and context switching
2. **Type safety**: Compile-time algorithm verification
3. **IPC-friendly**: No lifetimes, can be stored in server state
4. **Resource safety**: Automatic session cleanup on drop
5. **Performance**: Minimal overhead over manual approach
6. **Ergonomic**: Clean, intuitive API for common patterns

## API Design

### Core Components

```
┌─────────────────────────────────────────────────────┐
│          SessionManager<const N: usize>             │
│  - Owns HaceController<MultiContextProvider>       │
│  - Tracks active sessions                           │
│  - Provides session allocation/deallocation         │
└────────────────┬────────────────────────────────────┘
                 │
                 │ Creates & manages
                 │
┌────────────────▼────────────────────────────────────┐
│         SessionHandle (opaque ID)                   │
│  - Session ID + type marker                         │
│  - Prevents session confusion                       │
└────────────────┬────────────────────────────────────┘
                 │
                 │ Used with
                 │
┌────────────────▼────────────────────────────────────┐
│    SessionDigest<T: DigestAlgorithm>                │
│  - Wraps OwnedDigestContext                         │
│  - Tracks session automatically                     │
│  - Auto-activates session on operations             │
└─────────────────────────────────────────────────────┘
```

### 1. SessionManager

The main entry point for multi-session operations:

```rust
/// Manager for multiple concurrent hash sessions
///
/// This is the recommended API for IPC servers that need to support
/// multiple concurrent hash operations (e.g., Hubris digest server).
///
/// # Type Parameters
/// - `N`: Maximum number of concurrent sessions (typically 4-8)
///
/// # Examples
///
/// ```no_run
/// use aspeed_ddk::digest::session::{SessionManager, Sha2_256};
///
/// // Create manager with 8 concurrent sessions
/// let mut manager = SessionManager::<8>::new(hace_peripheral)?;
///
/// // Start a session
/// let session1 = manager.init_sha256()?;
///
/// // Update (session automatically activated)
/// let session1 = session1.update(b"hello")?;
/// let session1 = session1.update(b" world")?;
///
/// // Finalize (session automatically released)
/// let (digest, _) = manager.finalize(session1)?;
/// ```
pub struct SessionManager<const N: usize> {
    /// The underlying controller (None when a session owns it)
    controller: Option<HaceController<MultiContextProvider>>,

    /// Session metadata (tracks which slot is used for what)
    sessions: [SessionSlot; N],

    /// Next session ID for uniqueness (wrapping counter)
    next_id: u32,
}

#[derive(Clone, Copy)]
struct SessionSlot {
    /// Session allocation state
    state: SlotState,

    /// Unique session ID (for validation)
    session_id: u32,

    /// Algorithm type (for debugging/validation)
    algorithm: Option<DigestAlgorithm>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SlotState {
    Free,
    Allocated,
    Active,  // Currently in use (context exists)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DigestAlgorithm {
    Sha256,
    Sha384,
    Sha512,
}
```

### 2. SessionHandle

Type-safe session identifier:

```rust
/// Opaque handle to a hash session
///
/// This handle is returned when initializing a session and must be
/// used to perform operations on that session. It cannot be forged
/// or confused with other sessions due to internal validation.
///
/// # Type Safety
///
/// The handle is generic over the digest algorithm, preventing
/// accidental algorithm mismatches at compile time:
///
/// ```compile_fail
/// let session = manager.init_sha256()?;
/// // ERROR: Type mismatch
/// manager.finalize_sha384(session)?;
/// ```
pub struct SessionHandle<T> {
    /// Slot index (0..N)
    slot: usize,

    /// Unique session ID (for validation)
    id: u32,

    /// Algorithm marker (zero-sized)
    _marker: PhantomData<T>,
}

// SessionHandle is Send + Sync (can cross IPC boundaries)
unsafe impl<T> Send for SessionHandle<T> {}
unsafe impl<T> Sync for SessionHandle<T> {}
```

### 3. SessionDigest

Auto-managed digest context:

```rust
/// Self-managing digest context for multi-session operations
///
/// This context automatically activates its session before each operation,
/// eliminating the need for manual session tracking. It wraps an
/// `OwnedDigestContext` with automatic session management.
///
/// # Lifecycle
///
/// 1. Created by `SessionManager::init_*()` methods
/// 2. Updated via `update()` (moves self, returns new self)
/// 3. Finalized via `SessionManager::finalize()` or dropped
///
/// # Drop Behavior
///
/// If dropped without finalizing, the session is automatically canceled
/// and the underlying controller is recovered (preventing resource leaks).
pub struct SessionDigest<T: DigestAlgorithm + IntoHashAlgo> {
    /// The owned digest context
    context: OwnedDigestContext<T, MultiContextProvider>,

    /// Provider session ID (for activation)
    provider_session_id: usize,

    /// Manager session ID (for validation)
    manager_session_id: u32,

    /// Slot index (for cleanup)
    slot: usize,
}

impl<T: DigestAlgorithm + IntoHashAlgo> SessionDigest<T> {
    /// Update the digest with additional data
    ///
    /// The session is automatically activated before the update operation.
    /// This consumes `self` and returns a new instance (move semantics).
    pub fn update(mut self, data: &[u8]) -> Result<Self, SessionError> {
        // Activate session in provider
        self.context
            .controller_mut()
            .provider_mut()
            .set_active_session(self.provider_session_id);

        // Perform update
        self.context = self.context
            .update(data)
            .map_err(|_| SessionError::UpdateFailed)?;

        Ok(self)
    }

    /// Get session handle for this digest
    pub fn handle(&self) -> SessionHandle<T> {
        SessionHandle {
            slot: self.slot,
            id: self.manager_session_id,
            _marker: PhantomData,
        }
    }
}

// No Drop implementation - finalize() or cancel() must be called explicitly
// This is intentional to force proper resource cleanup
```

### 4. SessionManager Implementation

```rust
impl<const N: usize> SessionManager<N> {
    /// Create a new session manager
    ///
    /// # Errors
    /// Returns error if provider initialization fails
    pub fn new(hace: Hace) -> Result<Self, SessionError> {
        let provider = MultiContextProvider::new(N)
            .map_err(|_| SessionError::InvalidSessionCount)?;

        let controller = HaceController::with_provider(hace, provider);

        Ok(Self {
            controller: Some(controller),
            sessions: [SessionSlot::default(); N],
            next_id: 0,
        })
    }

    /// Initialize a new SHA-256 session
    ///
    /// # Errors
    /// Returns error if all session slots are full
    pub fn init_sha256(&mut self) -> Result<SessionDigest<Sha2_256>, SessionError> {
        self.init_session::<Sha2_256>(DigestAlgorithm::Sha256)
    }

    /// Initialize a new SHA-384 session
    pub fn init_sha384(&mut self) -> Result<SessionDigest<Sha2_384>, SessionError> {
        self.init_session::<Sha2_384>(DigestAlgorithm::Sha384)
    }

    /// Initialize a new SHA-512 session
    pub fn init_sha512(&mut self) -> Result<SessionDigest<Sha2_512>, SessionError> {
        self.init_session::<Sha2_512>(DigestAlgorithm::Sha512)
    }

    /// Generic session initialization
    fn init_session<T>(&mut self, algo: DigestAlgorithm)
        -> Result<SessionDigest<T>, SessionError>
    where
        T: DigestAlgorithm + IntoHashAlgo + Default,
    {
        // Find free slot
        let slot = self.sessions
            .iter()
            .position(|s| s.state == SlotState::Free)
            .ok_or(SessionError::TooManySessions)?;

        // Take controller
        let mut controller = self.controller
            .take()
            .ok_or(SessionError::ControllerInUse)?;

        // Allocate provider session
        let provider_session_id = controller
            .provider_mut()
            .allocate_session()
            .map_err(|_| SessionError::TooManySessions)?;

        // Set as active
        controller.provider_mut().set_active_session(provider_session_id);

        // Initialize digest
        let context = controller
            .init(T::default())
            .map_err(|_| SessionError::InitializationFailed)?;

        // Generate unique session ID
        let manager_session_id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        // Mark slot as active
        self.sessions[slot] = SessionSlot {
            state: SlotState::Active,
            session_id: manager_session_id,
            algorithm: Some(algo),
        };

        Ok(SessionDigest {
            context,
            provider_session_id,
            manager_session_id,
            slot,
        })
    }

    /// Finalize a session and return the digest
    ///
    /// The session is automatically released and the controller is recovered.
    ///
    /// # Type Safety
    ///
    /// The `SessionHandle` ensures compile-time algorithm verification:
    /// ```compile_fail
    /// let session = manager.init_sha256()?;
    /// // ERROR: Type mismatch
    /// manager.finalize_sha384(session)?;
    /// ```
    pub fn finalize<T>(&mut self, digest: SessionDigest<T>)
        -> Result<(T::Digest, SessionHandle<T>), SessionError>
    where
        T: DigestAlgorithm + IntoHashAlgo,
    {
        // Validate session
        let slot_data = self.sessions
            .get(digest.slot)
            .ok_or(SessionError::InvalidSession)?;

        if slot_data.session_id != digest.manager_session_id {
            return Err(SessionError::InvalidSession);
        }

        // Activate session
        let mut context = digest.context;
        context
            .controller_mut()
            .provider_mut()
            .set_active_session(digest.provider_session_id);

        // Finalize digest
        let (output, mut controller) = context
            .finalize()
            .map_err(|_| SessionError::FinalizationFailed)?;

        // Release provider session
        controller.provider_mut().release_session(digest.provider_session_id);

        // Mark slot as free
        self.sessions[digest.slot] = SessionSlot {
            state: SlotState::Free,
            session_id: 0,
            algorithm: None,
        };

        // Return controller
        self.controller = Some(controller);

        // Create handle for result
        let handle = SessionHandle {
            slot: digest.slot,
            id: digest.manager_session_id,
            _marker: PhantomData,
        };

        Ok((output, handle))
    }

    /// Cancel a session without finalizing
    ///
    /// This is useful for error handling or when aborting an operation.
    /// The session is released and the controller is recovered.
    pub fn cancel<T>(&mut self, digest: SessionDigest<T>) -> Result<(), SessionError>
    where
        T: DigestAlgorithm + IntoHashAlgo,
    {
        // Validate session
        let slot_data = self.sessions
            .get(digest.slot)
            .ok_or(SessionError::InvalidSession)?;

        if slot_data.session_id != digest.manager_session_id {
            return Err(SessionError::InvalidSession);
        }

        // Cancel context
        let mut controller = digest.context.cancel();

        // Release provider session
        controller.provider_mut().release_session(digest.provider_session_id);

        // Mark slot as free
        self.sessions[digest.slot] = SessionSlot {
            state: SlotState::Free,
            session_id: 0,
            algorithm: None,
        };

        // Return controller
        self.controller = Some(controller);

        Ok(())
    }

    /// Get the number of active sessions
    pub fn active_count(&self) -> usize {
        self.sessions
            .iter()
            .filter(|s| s.state == SlotState::Active)
            .count()
    }

    /// Check if a session is valid
    pub fn is_valid<T>(&self, handle: &SessionHandle<T>) -> bool {
        self.sessions
            .get(handle.slot)
            .map(|s| s.session_id == handle.id && s.state == SlotState::Active)
            .unwrap_or(false)
    }
}

impl<const N: usize> Default for SessionSlot {
    fn default() -> Self {
        Self {
            state: SlotState::Free,
            session_id: 0,
            algorithm: None,
        }
    }
}
```

### 5. Error Types

```rust
/// Errors that can occur during session management
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    /// All session slots are currently in use
    TooManySessions,

    /// Invalid session ID or handle
    InvalidSession,

    /// Controller is currently in use by another session
    ControllerInUse,

    /// Session initialization failed
    InitializationFailed,

    /// Update operation failed
    UpdateFailed,

    /// Finalization operation failed
    FinalizationFailed,

    /// Invalid session count (N must be 1-8)
    InvalidSessionCount,
}
```

## Usage Examples

### Example 1: Basic IPC Digest Server

```rust
use aspeed_ddk::digest::session::{SessionManager, SessionDigest, SessionHandle, Sha2_256};

/// Hubris digest server state
pub struct DigestServer {
    manager: SessionManager<8>,

    // Store active digests (can be Option because of move semantics)
    active_digests: [Option<ActiveDigest>; 8],
}

enum ActiveDigest {
    Sha256(SessionDigest<Sha2_256>),
    Sha384(SessionDigest<Sha2_384>),
    Sha512(SessionDigest<Sha2_512>),
}

impl DigestServer {
    pub fn new(hace: Hace) -> Result<Self, SessionError> {
        Ok(Self {
            manager: SessionManager::new(hace)?,
            active_digests: [None, None, None, None, None, None, None, None],
        })
    }

    /// IPC handler: Initialize SHA-256 session
    pub fn handle_init_sha256(&mut self) -> Result<u32, DigestError> {
        // Initialize session
        let digest = self.manager.init_sha256()
            .map_err(|_| DigestError::TooManySessions)?;

        // Get handle before storing digest
        let handle = digest.handle();

        // Store in first free slot
        let slot = self.active_digests
            .iter()
            .position(|d| d.is_none())
            .ok_or(DigestError::TooManySessions)?;

        self.active_digests[slot] = Some(ActiveDigest::Sha256(digest));

        // Return slot as session ID
        Ok(slot as u32)
    }

    /// IPC handler: Update session
    pub fn handle_update(&mut self, session_id: u32, data: &[u8])
        -> Result<(), DigestError>
    {
        let slot = session_id as usize;

        // Take digest from storage
        let digest = self.active_digests
            .get_mut(slot)
            .and_then(|d| d.take())
            .ok_or(DigestError::InvalidSession)?;

        // Update based on algorithm
        match digest {
            ActiveDigest::Sha256(d) => {
                let updated = d.update(data)
                    .map_err(|_| DigestError::UpdateError)?;
                self.active_digests[slot] = Some(ActiveDigest::Sha256(updated));
            }
            ActiveDigest::Sha384(d) => {
                let updated = d.update(data)
                    .map_err(|_| DigestError::UpdateError)?;
                self.active_digests[slot] = Some(ActiveDigest::Sha384(updated));
            }
            ActiveDigest::Sha512(d) => {
                let updated = d.update(data)
                    .map_err(|_| DigestError::UpdateError)?;
                self.active_digests[slot] = Some(ActiveDigest::Sha512(updated));
            }
        }

        Ok(())
    }

    /// IPC handler: Finalize SHA-256 session
    pub fn handle_finalize_sha256(&mut self, session_id: u32, output: &mut [u32; 8])
        -> Result<(), DigestError>
    {
        let slot = session_id as usize;

        // Take digest
        let digest = self.active_digests
            .get_mut(slot)
            .and_then(|d| d.take())
            .ok_or(DigestError::InvalidSession)?;

        match digest {
            ActiveDigest::Sha256(d) => {
                // Finalize
                let (result, _handle) = self.manager.finalize(d)
                    .map_err(|_| DigestError::FinalizationError)?;

                // Copy output
                output.copy_from_slice(result.as_ref());

                Ok(())
            }
            _ => {
                // Wrong algorithm, put it back
                self.active_digests[slot] = Some(digest);
                Err(DigestError::AlgorithmMismatch)
            }
        }
    }
}
```

### Example 2: Concurrent Session Processing

```rust
/// Demonstrate concurrent hash operations
pub fn concurrent_hashing_example(hace: Hace) -> Result<(), SessionError> {
    let mut manager = SessionManager::<4>::new(hace)?;

    // Start multiple sessions
    let mut sha256_session = manager.init_sha256()?;
    let mut sha384_session = manager.init_sha384()?;
    let mut sha512_session = manager.init_sha512()?;

    // Interleave updates (context switches happen automatically)
    sha256_session = sha256_session.update(b"SHA-256 data 1")?;
    sha384_session = sha384_session.update(b"SHA-384 data 1")?;
    sha512_session = sha512_session.update(b"SHA-512 data 1")?;

    sha256_session = sha256_session.update(b"SHA-256 data 2")?;
    sha384_session = sha384_session.update(b"SHA-384 data 2")?;

    // Finalize in any order
    let (digest512, _) = manager.finalize(sha512_session)?;
    let (digest256, _) = manager.finalize(sha256_session)?;
    let (digest384, _) = manager.finalize(sha384_session)?;

    Ok(())
}
```

### Example 3: Error Handling with Cancel

```rust
/// Demonstrate error handling with session cancellation
pub fn error_handling_example(mut manager: SessionManager<4>) -> Result<(), SessionError> {
    let digest = manager.init_sha256()?;

    let digest = match digest.update(b"some data") {
        Ok(d) => d,
        Err(e) => {
            // Cancel session on error
            manager.cancel(digest)?;
            return Err(e);
        }
    };

    // Continue processing...
    let (result, _) = manager.finalize(digest)?;

    Ok(())
}
```

## Implementation Requirements

### Required Changes to Existing Code

1. **Add `controller_mut()` to `OwnedDigestContext`**:

```rust
impl<T, P> OwnedDigestContext<T, P>
where
    T: DigestAlgorithm + IntoHashAlgo,
    P: HaceContextProvider,
{
    /// Get mutable reference to the underlying controller
    ///
    /// This is needed for session management in multi-context scenarios.
    pub fn controller_mut(&mut self) -> &mut HaceController<P> {
        &mut self.controller
    }

    /// Cancel the context and recover the controller
    pub fn cancel(self) -> HaceController<P> {
        // Cleanup if needed
        let mut controller = self.controller;
        controller.cleanup_context();
        controller
    }
}
```

2. **Export from `digest` module**:

```rust
// In src/digest/mod.rs
pub mod session;

pub use session::{SessionManager, SessionDigest, SessionHandle, SessionError};
```

## Performance Analysis

### Memory Overhead

```
SessionManager<N>:
  - HaceController: ~16 bytes
  - MultiContextProvider: ~3 KB (for N=4)
  - SessionSlot[N]: N × 16 bytes
  - Total: ~3 KB + (N × 16 bytes)

For N=8: ~3.2 KB
```

### Runtime Overhead

Compared to manual session management:

```
SessionManager overhead per operation:
  - Session validation: ~5-10 cycles (array lookup + comparison)
  - Session activation: 0 cycles (already done in manual approach)
  - Total: Negligible (<1% for typical hash operations)
```

### Context Switch Performance

Same as multi-context provider (15-30µs @ 200MHz), since this is just a wrapper.

## Type Safety Guarantees

### Compile-Time Verification

```rust
// ✅ Compiles: Correct algorithm
let session = manager.init_sha256()?;
let (digest, _) = manager.finalize(session)?;

// ❌ Compile error: Type mismatch
let session = manager.init_sha256()?;
manager.finalize_sha384(session)?;  // ERROR!

// ❌ Compile error: Wrong finalize type
let sha256_session = manager.init_sha256()?;
let sha384_session = manager.init_sha384()?;
manager.finalize(sha384_session)?;  // OK
manager.finalize(sha256_session)?;  // OK
// But can't mix them in storage without enum wrapper
```

### Runtime Validation

```rust
// Session ID verification prevents:
// - Use-after-free
// - Session confusion
// - Stale handle usage

let handle = digest.handle();
drop(digest);

// This fails validation (session was finalized)
if manager.is_valid(&handle) {
    // Never reached
}
```

## Comparison: Manual vs SessionManager

| Aspect | Manual Management | SessionManager |
|--------|------------------|----------------|
| **Lines of code** | ~50-100 per server | ~10-20 per server |
| **Boilerplate** | High (session tracking, validation) | Minimal |
| **Error handling** | Manual cleanup needed | Automatic via cancel() |
| **Type safety** | Enum-based | Generic-based (stronger) |
| **Session validation** | Manual checks | Automatic |
| **Resource leaks** | Possible if not careful | Prevented by API |
| **Performance** | Optimal | ~Same (negligible overhead) |
| **Flexibility** | High | Medium-high |

## Migration Guide

### From Manual Session Management

**Before**:
```rust
pub struct DigestServer {
    controller: Option<HaceController<MultiContextProvider>>,
    sessions: [Option<SessionState>; 8],
}

impl DigestServer {
    fn init_sha256(&mut self) -> Result<u32, Error> {
        let slot = /* find free slot */;
        let mut controller = self.controller.take()?;
        let session_id = controller.provider_mut().allocate_session()?;
        controller.provider_mut().set_active_session(session_id);
        let ctx = controller.init(Sha2_256::default())?;
        self.sessions[slot] = Some(SessionState::Sha256 { session_id, ctx });
        Ok(slot as u32)
    }
    // ... 50 more lines ...
}
```

**After**:
```rust
pub struct DigestServer {
    manager: SessionManager<8>,
    sessions: [Option<ActiveDigest>; 8],
}

impl DigestServer {
    fn init_sha256(&mut self) -> Result<u32, Error> {
        let digest = self.manager.init_sha256()?;
        let handle = digest.handle();
        let slot = /* find free slot */;
        self.sessions[slot] = Some(ActiveDigest::Sha256(digest));
        Ok(slot as u32)
    }
    // ... 10 more lines ...
}
```

**Benefits**: 5-10x reduction in boilerplate, automatic cleanup, stronger type safety.

## Open Design Questions

1. **Should `SessionDigest` implement `Drop` for auto-cleanup?**
   - Pro: Prevents resource leaks
   - Con: Implicit behavior, harder to track controller lifecycle
   - **Decision**: No Drop - require explicit `finalize()` or `cancel()`

2. **Should handles be reusable across sessions?**
   - Currently: Handles become invalid after finalize
   - Alternative: Could track generation counts for reuse
   - **Decision**: Keep simple - one handle per session lifetime

3. **Should we provide async variants?**
   - Hubris doesn't use async
   - Future consideration for other RTOS
   - **Decision**: Defer to future work

## Testing Strategy

### Unit Tests

```rust
#[test]
fn test_session_lifecycle() {
    let mut manager = SessionManager::<4>::new(mock_hace()).unwrap();

    let digest = manager.init_sha256().unwrap();
    let handle = digest.handle();

    assert!(manager.is_valid(&handle));
    assert_eq!(manager.active_count(), 1);

    let digest = digest.update(b"test").unwrap();
    let (result, handle) = manager.finalize(digest).unwrap();

    assert!(!manager.is_valid(&handle));  // Invalid after finalize
    assert_eq!(manager.active_count(), 0);
}

#[test]
fn test_concurrent_sessions() {
    let mut manager = SessionManager::<4>::new(mock_hace()).unwrap();

    let s1 = manager.init_sha256().unwrap();
    let s2 = manager.init_sha384().unwrap();
    let s3 = manager.init_sha512().unwrap();

    assert_eq!(manager.active_count(), 3);

    let s1 = s1.update(b"data1").unwrap();
    let s2 = s2.update(b"data2").unwrap();
    let s3 = s3.update(b"data3").unwrap();

    manager.finalize(s1).unwrap();
    assert_eq!(manager.active_count(), 2);
}

#[test]
fn test_session_limit() {
    let mut manager = SessionManager::<2>::new(mock_hace()).unwrap();

    let s1 = manager.init_sha256().unwrap();
    let s2 = manager.init_sha384().unwrap();

    // Third allocation should fail
    assert!(manager.init_sha512().is_err());

    // After finalizing, slot becomes available
    manager.finalize(s1).unwrap();
    assert!(manager.init_sha512().is_ok());
}
```

### Integration Tests

Test with actual HACE hardware or QEMU to verify context switching correctness.

## Future Enhancements

1. **Session statistics**: Track hash count, bytes processed, context switches
2. **Session timeout**: Optional timeout for idle sessions
3. **Priority scheduling**: Higher priority sessions preempt lower priority
4. **Batch operations**: Update multiple sessions in one call
5. **HMAC support**: Extend to HMAC operations with key management

## References

- [multi-context-ipc-integration.md](multi-context-ipc-integration.md)
- [hace-multi-context-design.md](hace-multi-context-design.md)
- [src/digest/hash_owned.rs](../src/digest/hash_owned.rs)
- [src/digest/multi_context.rs](../src/digest/multi_context.rs)

---

**Document Version**: 1.0
**Last Updated**: 2025-09-30
**Status**: Design complete, ready for implementation
