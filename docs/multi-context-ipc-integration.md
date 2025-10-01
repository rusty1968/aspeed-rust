# Multi-Context Integration for IPC-Based Digest Servers

**Date**: 2025-09-30
**Status**: Design Document
**Related**: [hace-multi-context-design.md](hace-multi-context-design.md), [hace-server-hubris.md](../hace-server-hubris.md)

## Executive Summary

This document discusses the integration of multi-context hash operations with IPC-based digest servers (Hubris), analyzing the trade-offs between scoped (borrowed) and owned API patterns, and proposing solutions for storing hash contexts across IPC call boundaries while supporting concurrent sessions.

**Key Finding**: The owned API (`hash_owned.rs`) has weaker natural affinity with multi-context switching than the scoped API (`hash.rs`), but is **required** for IPC persistence. A session-aware wrapper pattern resolves this tension.

## Background

### The Two API Patterns

aspeed-ddk provides two digest API implementations:

#### 1. Scoped API ([src/digest/hash.rs](../src/digest/hash.rs))

**Pattern**: Borrowed controller with lifetime constraints

```rust
pub struct OpContextImpl<'a, A, P> {
    controller: &'a mut HaceController<P>,  // Borrows controller
    _phantom: PhantomData<A>,
}

impl<A, P> DigestInit<A> for HaceController<P> {
    type OpContext<'a> = OpContextImpl<'a, A, P> where Self: 'a;

    fn init(&mut self, _algo: A) -> Result<Self::OpContext<'_>, ...> {
        // Returns borrowed context
        Ok(OpContextImpl { controller: self, ... })
    }
}

impl<A, P> DigestOp for OpContextImpl<'_, A, P> {
    fn update(&mut self, input: &[u8]) -> Result<(), ...>;
    fn finalize(self) -> Result<Self::Output, ...>;
}
```

**Characteristics**:
- Context **borrows** the controller (`&'a mut`)
- Controller remains accessible while contexts live
- Cannot cross IPC boundaries (lifetime constraints)
- Natural fit for multi-context scenarios

#### 2. Owned API ([src/digest/hash_owned.rs](../src/digest/hash_owned.rs))

**Pattern**: Move semantics with controller ownership transfer

```rust
pub struct OwnedDigestContext<T, P = SingleContextProvider> {
    controller: HaceController<P>,  // Owns controller
    _phantom: PhantomData<T>,
}

impl<P> DigestInit<Sha2_256> for HaceController<P> {
    type Context = OwnedDigestContext<Sha2_256, P>;

    fn init(mut self, ...) -> Result<Self::Context, ...> {
        // Consumes controller, returns owned context
        Ok(OwnedDigestContext { controller: self, ... })
    }
}

impl<P> DigestOp for OwnedDigestContext<Sha2_256, P> {
    fn update(mut self, data: &[u8]) -> Result<Self, ...>;
    fn finalize(self) -> Result<(Output, HaceController<P>), ...>;
}
```

**Characteristics**:
- Context **owns** the controller (moved)
- No lifetime constraints - can be stored in structs
- Controller recovered on `finalize()` or `cancel()`
- Required for IPC persistence (Hubris digest server)

### Multi-Context Provider

The [MultiContextProvider](../src/digest/multi_context.rs) enables concurrent hash operations:

```rust
pub struct MultiContextProvider {
    contexts: [MaybeUninit<AspeedHashContext>; MAX_SESSIONS],  // Storage
    allocated: [bool; MAX_SESSIONS],                            // Allocation bitmap
    active_id: usize,                                           // Current session
    last_loaded: Option<usize>,                                 // Context switch cache
}

impl HaceContextProvider for MultiContextProvider {
    fn ctx_mut(&mut self) -> Result<&mut AspeedHashContext, ...> {
        // Automatic lazy context switching
        if self.last_loaded != Some(self.active_id) {
            if let Some(prev_id) = self.last_loaded {
                self.save_hw_to_slot(prev_id)?;  // Save old context
            }
            self.load_slot_to_hw(self.active_id)?;  // Load new context
            self.last_loaded = Some(self.active_id);
        }

        // Return shared hardware context
        Ok(unsafe { &mut *shared_hash_ctx() })
    }
}
```

**Key behavior**: Context switching happens transparently in `ctx_mut()` based on `active_id`.

## The Affinity Problem

### Why Scoped API Has Strong Affinity with Multi-Context

The scoped API naturally supports multiple concurrent contexts:

```rust
let mut controller = HaceController::with_provider(hace, multi_provider);

// Allocate sessions
let session1 = controller.provider_mut().allocate_session()?;
let session2 = controller.provider_mut().allocate_session()?;

// Create multiple contexts simultaneously
controller.provider_mut().set_active_session(session1);
let mut ctx1 = controller.init(Sha256::default())?;

controller.provider_mut().set_active_session(session2);
let mut ctx2 = controller.init(Sha384::default())?;

// Interleave operations on different sessions
ctx1.update(b"data for session 1")?;
ctx2.update(b"data for session 2")?;
ctx1.update(b"more data for session 1")?;

// Both contexts finalize independently
let digest1 = ctx1.finalize()?;
let digest2 = ctx2.finalize()?;
```

**Why this works**:
1. `ctx1` and `ctx2` both hold `&mut HaceController<MultiContextProvider>`
2. Before each operation, the code can call `controller.provider_mut().set_active_session()`
3. When `ctx1.update()` runs, it calls `controller.ctx_mut()` → `provider.ctx_mut()` → context switch
4. Controller is **always accessible** through the mutable reference

**Problem**: Cannot cross IPC boundaries due to lifetime `'a`

### Why Owned API Has Weak Affinity with Multi-Context

The owned API moves the controller into the context:

```rust
let controller = HaceController::with_provider(hace, multi_provider);

// Allocate session
let session1 = controller.provider_mut().allocate_session()?;
controller.provider_mut().set_active_session(session1);

// Controller is MOVED into ctx1
let ctx1 = controller.init(Sha2_256::default())?;

// ❌ PROBLEM: Can't access controller anymore!
// ❌ Can't call controller.provider_mut().set_active_session(session2)
// ❌ Can't allocate more sessions
// ❌ Can't create ctx2 while ctx1 exists

// Must finalize to recover controller
let (digest1, controller) = ctx1.finalize()?;

// Now can create second session
let session2 = controller.provider_mut().allocate_session()?;
controller.provider_mut().set_active_session(session2);
let ctx2 = controller.init(Sha2_384::default())?;
```

**Fundamental limitation**: Only **one** owned context can exist at a time because the controller is moved.

### The IPC Constraint

Hubris digest servers must:
1. **Store contexts across IPC calls** (user calls `init()`, then later calls `update()`)
2. **Support multiple concurrent sessions** (multiple clients, each with independent state)

**IPC boundary requirement**: Contexts must be stored in the server's state struct:

```rust
pub struct DigestServer {
    // Must store contexts here (no lifetimes allowed!)
    active_sessions: [Option<???>, MAX_SESSIONS],
}

impl DigestServer {
    fn handle_init_sha256(&mut self) -> u32 {
        // Create context...
        // Store in self.active_sessions
        // Return session ID
    }

    fn handle_update(&mut self, session_id: u32, data: &[u8]) {
        // Retrieve context from self.active_sessions
        // Call update
        // Store back
    }
}
```

**Scoped API fails here**: Cannot store `OpContextImpl<'a, ...>` because of lifetime `'a`.

**Owned API required**: `OwnedDigestContext<T, P>` has no lifetimes, can be stored.

## The Integration Challenge

We need to reconcile:
- ✅ **IPC persistence requirement** → Owned API
- ✅ **Multi-session requirement** → MultiContextProvider
- ❌ **Owned API + MultiContextProvider** → Session management complexity

### Current Gap Analysis

The current implementation ([hash_owned.rs:52-68](../src/digest/hash_owned.rs#L52-L68)) makes `OwnedDigestContext` generic over provider:

```rust
pub struct OwnedDigestContext<
    T: DigestAlgorithm + IntoHashAlgo,
    P: HaceContextProvider = SingleContextProvider,  // ✅ Can use MultiContextProvider
> {
    controller: HaceController<P>,
    _phantom: PhantomData<T>,
}
```

**Problem identified**: The context owns the controller, but **doesn't track which session it's using**.

When you call `context.update(data)`:
1. Internally calls `self.controller.ctx_mut_unchecked()`
2. Which calls `self.controller.provider.ctx_mut()`
3. `MultiContextProvider::ctx_mut()` uses `self.active_id` for context switching
4. **But which session is `active_id`?** The `OwnedDigestContext` doesn't know!

### The Session Tracking Problem

Consider this scenario:

```rust
// Server has controller with MultiContextProvider
let mut controller = HaceController::with_provider(hace, multi_provider);

// Session 1: init SHA-256
let session1_id = controller.provider_mut().allocate_session()?;
controller.provider_mut().set_active_session(session1_id);
let ctx1 = controller.init(Sha2_256::default())?;  // ❌ controller moved!

// ❌ PROBLEM: Can't allocate session2 because controller is gone!
// ❌ Can't store ctx1 because we need controller for session2!
```

## Solution Approaches

### Approach 1: Manual Session Management (Current Workaround)

As shown in [hash_owned.rs:260-299](../src/digest/hash_owned.rs#L260-L299), manually manage sessions with `Option` storage:

```rust
struct DigestServer {
    // Store contexts in slots
    sessions: [Option<SessionState>; MAX_SESSIONS],
    // Keep controller separate when not in use
    controller: Option<HaceController<MultiContextProvider>>,
}

enum SessionState {
    Sha256 { ctx: OwnedDigestContext<Sha2_256, MultiContextProvider> },
    Sha384 { ctx: OwnedDigestContext<Sha2_384, MultiContextProvider> },
    // ...
}

impl DigestServer {
    fn init_sha256(&mut self) -> Result<u32, Error> {
        // Find free slot
        let slot_idx = self.sessions.iter().position(|s| s.is_none())?;

        // Take controller (must be None in sessions)
        let mut controller = self.controller.take()?;

        // Allocate session in provider
        let session_id = controller.provider_mut().allocate_session()?;
        controller.provider_mut().set_active_session(session_id);

        // Init consumes controller
        let ctx = controller.init(Sha2_256::default())?;

        // Store context (controller now inside ctx)
        self.sessions[slot_idx] = Some(SessionState::Sha256 { ctx });

        Ok(slot_idx as u32)
    }

    fn update(&mut self, handle: u32, data: &[u8]) -> Result<(), Error> {
        // Take session from storage
        let session = self.sessions[handle as usize].take()?;

        match session {
            SessionState::Sha256 { ctx } => {
                // ❌ CRITICAL BUG: Can't set active session!
                // The controller is inside ctx, we can't access provider!

                let ctx = ctx.update(data)?;
                self.sessions[handle as usize] = Some(SessionState::Sha256 { ctx });
            }
        }
        Ok(())
    }
}
```

**Fatal flaw**: Cannot call `set_active_session()` because controller is inside the context!

### Approach 2: Session-Aware Owned Context (Recommended)

Create a wrapper that tracks its session ID and sets it before operations:

```rust
/// Session-aware owned digest context for IPC servers
///
/// This wrapper combines an owned digest context with session tracking,
/// automatically setting the active session before each operation.
pub struct SessionDigestContext<T, P>
where
    T: DigestAlgorithm + IntoHashAlgo,
    P: HaceContextProvider,
{
    context: OwnedDigestContext<T, P>,
    session_id: usize,
}

impl<T, P> SessionDigestContext<T, P>
where
    T: DigestAlgorithm + IntoHashAlgo,
    P: HaceContextProvider,
{
    /// Create from an owned context and session ID
    pub fn new(context: OwnedDigestContext<T, P>, session_id: usize) -> Self {
        Self { context, session_id }
    }

    /// Get the session ID
    pub fn session_id(&self) -> usize {
        self.session_id
    }

    /// Update with automatic session activation
    pub fn update(mut self, data: &[u8]) -> Result<Self, Infallible>
    where
        P: SessionProvider,  // Trait for providers that support sessions
    {
        // Set active session before operation
        self.context.controller_mut().provider_mut().set_active_session(self.session_id);

        // Perform update (context switches automatically in ctx_mut())
        let context = self.context.update(data)?;

        Ok(Self { context, session_id: self.session_id })
    }

    /// Finalize with automatic session activation and cleanup
    pub fn finalize(mut self) -> Result<(T::Digest, HaceController<P>), Infallible>
    where
        P: SessionProvider,
    {
        // Set active session before operation
        self.context.controller_mut().provider_mut().set_active_session(self.session_id);

        // Finalize (returns digest and controller)
        let (digest, mut controller) = self.context.finalize()?;

        // Release session in provider
        controller.provider_mut().release_session(self.session_id);

        Ok((digest, controller))
    }

    /// Cancel and recover controller
    pub fn cancel(mut self) -> HaceController<P>
    where
        P: SessionProvider,
    {
        let mut controller = self.context.cancel();
        controller.provider_mut().release_session(self.session_id);
        controller
    }
}

/// Trait for providers that support session management
pub trait SessionProvider: HaceContextProvider {
    fn allocate_session(&mut self) -> Result<usize, SessionError>;
    fn release_session(&mut self, session_id: usize);
    fn set_active_session(&mut self, session_id: usize);
}

impl SessionProvider for MultiContextProvider {
    fn allocate_session(&mut self) -> Result<usize, SessionError> {
        MultiContextProvider::allocate_session(self)
    }

    fn release_session(&mut self, session_id: usize) {
        MultiContextProvider::release_session(self, session_id)
    }

    fn set_active_session(&mut self, session_id: usize) {
        MultiContextProvider::set_active_session(self, session_id)
    }
}
```

**Problem with this approach**: `OwnedDigestContext` doesn't expose `controller_mut()` - the controller is private!

### Approach 3: Provider-Managed Session Association (Best Solution)

Instead of the context tracking its session, make the **provider** track which session each context belongs to:

```rust
/// Enhanced multi-context provider with automatic session tracking
pub struct MultiContextProvider {
    // Existing fields...
    contexts: [MaybeUninit<AspeedHashContext>; MAX_SESSIONS],
    allocated: [bool; MAX_SESSIONS],
    active_id: usize,
    last_loaded: Option<usize>,

    // NEW: Track which context "slot" maps to which provider session
    // This is set once when a context is initialized
    context_sessions: [Option<usize>; MAX_SESSIONS],
}

impl MultiContextProvider {
    /// Begin a session-scoped operation
    ///
    /// Sets the active session and returns a guard that tracks it.
    /// The guard can be passed through the owned context operations.
    pub fn begin_session(&mut self, session_id: usize) -> SessionGuard<'_> {
        self.set_active_session(session_id);
        SessionGuard { provider: self, session_id }
    }
}

/// RAII guard that maintains active session during operations
pub struct SessionGuard<'a> {
    provider: &'a mut MultiContextProvider,
    session_id: usize,
}

impl Drop for SessionGuard<'_> {
    fn drop(&mut self) {
        // Optional: Could clear active session or track statistics
    }
}
```

**Problem**: Still requires access to provider before each operation, but controller is inside context!

### Approach 4: Split Controller Pattern (Cleanest Solution)

Separate the provider from the controller, allowing independent access:

```rust
/// IPC digest server with split controller/provider pattern
pub struct DigestServer {
    /// The provider (manages sessions, context switching)
    provider: MultiContextProvider,

    /// HACE peripheral reference (doesn't own provider)
    hace: Hace,

    /// Active sessions with their contexts
    sessions: [Option<SessionState>; MAX_SESSIONS],
}

enum SessionState {
    Sha256 {
        session_id: usize,
        ctx: OwnedDigestContext<Sha2_256, ()>,  // ✅ No provider in context!
    },
    Sha384 {
        session_id: usize,
        ctx: OwnedDigestContext<Sha2_384, ()>,
    },
    // ...
}

impl DigestServer {
    pub fn new(hace: Hace) -> Result<Self, SessionError> {
        Ok(Self {
            provider: MultiContextProvider::new(MAX_SESSIONS)?,
            hace,
            sessions: [None, None, None, None],
        })
    }

    pub fn init_sha256(&mut self) -> Result<u32, Error> {
        // Find free slot
        let slot_idx = self.sessions.iter().position(|s| s.is_none())?;

        // Allocate session in provider
        let session_id = self.provider.allocate_session()?;

        // Set as active
        self.provider.set_active_session(session_id);

        // Create temporary controller with borrowed provider
        let controller = HaceController::with_provider_ref(
            &self.hace,
            &mut self.provider
        );

        // Initialize (moves controller, but provider stays separate!)
        let ctx = controller.init(Sha2_256::default())?;

        // Store session
        self.sessions[slot_idx] = Some(SessionState::Sha256 { session_id, ctx });

        Ok(slot_idx as u32)
    }

    pub fn update(&mut self, handle: u32, data: &[u8]) -> Result<(), Error> {
        let slot = &mut self.sessions[handle as usize];
        let session = slot.take()?;

        match session {
            SessionState::Sha256 { session_id, ctx } => {
                // ✅ Set active session (provider is accessible!)
                self.provider.set_active_session(session_id);

                // Update (context switch happens in ctx_mut())
                let ctx = ctx.update(data)?;

                *slot = Some(SessionState::Sha256 { session_id, ctx });
            }
            // ... other algorithms
        }
        Ok(())
    }

    pub fn finalize_sha256(&mut self, handle: u32, digest_out: &mut [u32; 8])
        -> Result<(), Error>
    {
        let session = self.sessions[handle as usize].take()?;

        match session {
            SessionState::Sha256 { session_id, ctx } => {
                // Set active session
                self.provider.set_active_session(session_id);

                // Finalize
                let (digest, _controller) = ctx.finalize()?;
                digest_out.copy_from_slice(digest.as_ref());

                // Release session
                self.provider.release_session(session_id);

                Ok(())
            }
            _ => Err(Error::AlgorithmMismatch),
        }
    }
}
```

**Problem with Approach 4**: Requires refactoring `HaceController` to support provider-by-reference, which changes the fundamental architecture.

### Approach 5: Dual-Controller Pattern (Pragmatic Solution)

Use **two different controller instances** - one for managing sessions, one embedded in contexts:

```rust
pub struct DigestServer {
    /// Management controller (used for provider access only)
    /// Never used for hash operations
    mgmt_controller: HaceController<MultiContextProvider>,

    /// Active sessions with their own controller instances
    sessions: [Option<SessionState>; MAX_SESSIONS],
}

enum SessionState {
    Sha256 {
        session_id: usize,
        // Each context has its own controller REFERENCE to same provider
        ctx: OwnedDigestContext<Sha2_256, MultiContextProvider>,
    },
    // ...
}
```

**Problem**: Can't have two `HaceController` instances sharing the same provider (provider isn't `Clone`/`Rc`).

## Recommended Solution: Session-ID Tracking in Context

After analyzing all approaches, the cleanest solution is to **extend `OwnedDigestContext`** with optional session tracking:

```rust
/// Extended owned digest context with session awareness
pub struct OwnedDigestContext<
    T: DigestAlgorithm + IntoHashAlgo,
    P: HaceContextProvider = SingleContextProvider,
> {
    controller: HaceController<P>,
    _phantom: PhantomData<T>,

    /// Session ID for multi-context providers (None for single-context)
    session_id: Option<usize>,
}

impl<T, P> OwnedDigestContext<T, P>
where
    T: DigestAlgorithm + IntoHashAlgo,
    P: HaceContextProvider,
{
    /// Set the session ID for this context
    ///
    /// The session will be automatically activated before each operation.
    /// Only needed for multi-context providers.
    pub fn with_session_id(mut self, session_id: usize) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// Get the session ID if set
    pub fn session_id(&self) -> Option<usize> {
        self.session_id
    }

    /// Internal: Activate session before operation (if multi-context)
    fn activate_session(&mut self) {
        if let Some(session_id) = self.session_id {
            // TODO: Need a way to call provider.set_active_session()
            // This requires adding a method to HaceController
        }
    }
}

// Update DigestOp implementation to activate session
impl<P> DigestOp for OwnedDigestContext<Sha2_256, P>
where
    P: HaceContextProvider,
{
    fn update(mut self, data: &[u8]) -> Result<Self, Self::Error> {
        self.activate_session();  // ✅ Automatic session activation

        // ... existing update logic ...

        Ok(self)
    }

    fn finalize(mut self) -> Result<(Self::Output, HaceController<P>), Self::Error> {
        self.activate_session();  // ✅ Automatic session activation

        // ... existing finalize logic ...

        // Release session if multi-context
        if let Some(session_id) = self.session_id {
            // TODO: controller.provider.release_session(session_id);
        }

        Ok((output, self.controller))
    }
}
```

### Required Changes to Enable This

1. **Add provider accessor to `HaceController`**:

```rust
impl<P: HaceContextProvider> HaceController<P> {
    /// Get mutable access to the provider (for session management)
    pub fn provider_mut(&mut self) -> &mut P {
        &mut self.provider
    }
}
```

2. **Add session activation helper**:

```rust
impl<T, P> OwnedDigestContext<T, P>
where
    T: DigestAlgorithm + IntoHashAlgo,
    P: HaceContextProvider,
{
    fn activate_session(&mut self) {
        if let Some(session_id) = self.session_id {
            // Check if provider supports session management
            // This is a bit awkward because we need a trait bound
            // For now, we can use an if-let pattern with downcasting
            // or require a SessionProvider trait bound
        }
    }
}
```

3. **Make provider session-aware**:

```rust
/// Trait for providers that support session management
pub trait SessionAware {
    fn set_active_session(&mut self, session_id: usize);
    fn release_session(&mut self, session_id: usize);
}

impl SessionAware for MultiContextProvider {
    fn set_active_session(&mut self, session_id: usize) {
        MultiContextProvider::set_active_session(self, session_id)
    }

    fn release_session(&mut self, session_id: usize) {
        MultiContextProvider::release_session(self, session_id)
    }
}

// SingleContextProvider is not session-aware (no-op)
impl SessionAware for SingleContextProvider {
    fn set_active_session(&mut self, _session_id: usize) {}
    fn release_session(&mut self, _session_id: usize) {}
}
```

### Final IPC Server Pattern

With these changes, the IPC server becomes clean and intuitive:

```rust
pub struct DigestServer {
    controller: Option<HaceController<MultiContextProvider>>,
    sessions: [Option<SessionState>; MAX_SESSIONS],
}

enum SessionState {
    Sha256 { ctx: OwnedDigestContext<Sha2_256, MultiContextProvider> },
    Sha384 { ctx: OwnedDigestContext<Sha2_384, MultiContextProvider> },
    Sha512 { ctx: OwnedDigestContext<Sha2_512, MultiContextProvider> },
}

impl DigestServer {
    pub fn init_sha256(&mut self) -> Result<u32, Error> {
        let slot_idx = self.sessions.iter().position(|s| s.is_none())?;

        let mut controller = self.controller.take()?;
        let session_id = controller.provider_mut().allocate_session()?;

        let ctx = controller
            .init(Sha2_256::default())?
            .with_session_id(session_id);  // ✅ Set session ID

        self.sessions[slot_idx] = Some(SessionState::Sha256 { ctx });
        Ok(slot_idx as u32)
    }

    pub fn update(&mut self, handle: u32, data: &[u8]) -> Result<(), Error> {
        let slot = &mut self.sessions[handle as usize];
        let session = slot.take()?;

        match session {
            SessionState::Sha256 { ctx } => {
                // ✅ Session automatically activated in update()
                let ctx = ctx.update(data)?;
                *slot = Some(SessionState::Sha256 { ctx });
            }
            // ...
        }
        Ok(())
    }

    pub fn finalize_sha256(&mut self, handle: u32, digest_out: &mut [u32; 8])
        -> Result<(), Error>
    {
        let session = self.sessions[handle as usize].take()?;

        match session {
            SessionState::Sha256 { ctx } => {
                // ✅ Session automatically activated and released in finalize()
                let (digest, controller) = ctx.finalize()?;
                digest_out.copy_from_slice(digest.as_ref());
                self.controller = Some(controller);
                Ok(())
            }
            _ => Err(Error::AlgorithmMismatch),
        }
    }
}
```

## Performance Considerations

### Context Switch Overhead

From [hace-multi-context-design.md](hace-multi-context-design.md):

- Context size: ~732 bytes
- Context switch: ~1,464 bytes copied (save + restore)
- Estimated time: 15-30 µs @ 200 MHz

### Lazy Switching Optimization

Context switches only occur when:
1. Active session changes
2. An operation calls `ctx_mut()`

Multiple updates on same session = zero context switches:

```rust
// Session 1: Three updates, ZERO switches
ctx1 = ctx1.update(data1)?;  // No switch (already active)
ctx1 = ctx1.update(data2)?;  // No switch
ctx1 = ctx1.update(data3)?;  // No switch

// Session 2: First update triggers ONE switch
ctx2 = ctx2.update(data4)?;  // Switch from session1 to session2
```

### Memory Footprint

```
Single context:  732 bytes
Multi-context:   732 × N + overhead

For N=4 sessions:
  Contexts:      2,928 bytes
  Metadata:      ~100 bytes (allocation bitmap, etc.)
  Total:         ~3 KB
```

## Security Considerations

### Context Isolation

- Each session has independent state
- Context switches use safe memory copies (bounds-checked)
- Session release performs secure zeroing (volatile writes)

From [multi_context.rs:125-135](../src/digest/multi_context.rs#L125-L135):

```rust
// Volatile zeroing to prevent compiler optimization
unsafe {
    let ctx_ptr = ctx.as_mut_ptr().cast::<u8>();
    let size = core::mem::size_of::<AspeedHashContext>();
    for i in 0..size {
        core::ptr::write_volatile(ctx_ptr.add(i), 0);
    }
}
```

### Session ID Validation

Proper validation prevents session confusion attacks:

```rust
debug_assert!(session_id < self.max_sessions, "Session ID out of bounds");
debug_assert!(
    self.allocated.get(session_id).copied().unwrap_or(false),
    "Session ID not allocated"
);
```

## Comparison Matrix

| Criterion | Scoped API | Owned API (Single) | Owned API (Multi) |
|-----------|------------|-------------------|-------------------|
| **IPC storage** | ❌ Lifetime constraints | ✅ No lifetimes | ✅ No lifetimes |
| **Multiple concurrent contexts** | ✅ Natural | ❌ One at a time | ⚠️ Requires session tracking |
| **Context switching** | ✅ Transparent | N/A | ✅ Transparent (with session ID) |
| **API complexity** | Simple | Simple | Moderate (session management) |
| **Memory overhead** | Minimal | Minimal | ~3 KB for 4 sessions |
| **Performance** | Best | Best | Good (15-30µs switch) |
| **Use case fit** | Embedded (no IPC) | Simple IPC | Multi-session IPC |

## Recommendations

### For Single-Session IPC Servers

Use owned API with `SingleContextProvider` (current default):

```rust
let controller = HaceController::new(hace);
let ctx = controller.init(Sha2_256::default())?;
// Store ctx in server state
```

### For Multi-Session IPC Servers (Hubris Digest Server)

1. **Immediate term**: Use manual session tracking pattern (Approach 1 variant)
2. **Short term**: Implement session ID tracking in `OwnedDigestContext` (Recommended Solution)
3. **Long term**: Consider dedicated multi-session API if pattern becomes common

### Implementation Priority

1. **High priority**: Add `provider_mut()` accessor to `HaceController` (already exists per line 191 in hace_controller.rs)
2. **High priority**: Add optional `session_id` field to `OwnedDigestContext`
3. **Medium priority**: Implement `SessionAware` trait for automatic session activation
4. **Low priority**: Add convenience wrappers for common IPC patterns

## Open Questions

1. **Should session activation be automatic or explicit?**
   - Automatic: Less boilerplate, harder to debug
   - Explicit: More verbose, clearer control flow

2. **Should `finalize()` automatically release the session?**
   - Pro: Prevents session leaks
   - Con: Less control for complex scenarios

3. **How to handle session activation errors?**
   - Current design ignores errors in `ctx_mut()` (per project requirements)
   - Should we add debug assertions?

4. **Should we provide a `MultiSessionController` wrapper?**
   - Could hide complexity behind a cleaner API
   - Trades flexibility for convenience

## Conclusion

The owned API is **required** for IPC persistence, but has **weaker affinity** with multi-context switching compared to the scoped API. This tension is resolved by:

1. Adding session ID tracking to `OwnedDigestContext`
2. Implementing automatic session activation in `update()`/`finalize()`
3. Using lazy context switching to minimize overhead

The resulting pattern is slightly more complex than single-context scenarios, but provides the necessary functionality for multi-session IPC servers like Hubris digest server while maintaining type safety and memory safety guarantees.

## References

- [hace-multi-context-design.md](hace-multi-context-design.md) - Multi-context architecture
- [hace-server-hubris.md](../hace-server-hubris.md) - Hubris digest server design
- [src/digest/hash.rs](../src/digest/hash.rs) - Scoped API implementation
- [src/digest/hash_owned.rs](../src/digest/hash_owned.rs) - Owned API implementation
- [src/digest/multi_context.rs](../src/digest/multi_context.rs) - Multi-context provider
- [src/hace_controller.rs](../src/hace_controller.rs) - HACE hardware controller

---

**Document Version**: 1.0
**Last Updated**: 2025-09-30
**Status**: Design document for implementation planning
