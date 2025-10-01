// Licensed under the Apache-2.0 license

//! Multi-context provider for concurrent hash operations
//!
//! This module implements `MultiContextProvider` which manages multiple independent
//! hash contexts, enabling concurrent hash operations required by security protocols.
//!
//! ## Design
//!
//! - Stores N independent `AspeedHashContext` instances
//! - Automatically switches contexts when accessing different sessions
//! - Uses lazy context switching (only switches when necessary)
//! - Context switches involve copying ~732 bytes of state
//!
//! ## Error Type
//!
//! This module uses `SessionError` as the error type for fallible operations.
//!
//! ## Usage
//!
//! ```no_run
//! use aspeed_ddk::digest::hace_controller::HaceController;
//! use aspeed_ddk::digest::multi_context::MultiContextProvider;
//!
//! // Create controller with multi-context support
//! let provider = MultiContextProvider::new(8); // 8 concurrent sessions
//! let mut controller = HaceController::with_provider(hace, provider);
//!
//! // Allocate sessions
//! let session1 = controller.provider_mut().allocate_session().unwrap();
//! let session2 = controller.provider_mut().allocate_session().unwrap();
//!
//! // Use sessions - context switches happen automatically
//! controller.provider_mut().set_active_session(session1);
//! // ... perform hash operations ...
//!
//! controller.provider_mut().set_active_session(session2);
//! // ... perform hash operations ...
//! ```

use super::hace_controller::AspeedHashContext;
use super::traits::HaceContextProvider;
use core::mem::MaybeUninit;

/// Error type for session allocation operations
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SessionError;

/// Maximum number of concurrent hash sessions supported
pub const MAX_SESSIONS: usize = 4;

/// Manages multiple hash contexts with automatic switching
pub struct MultiContextProvider {
    /// Stored context states (one per session)
    contexts: [MaybeUninit<AspeedHashContext>; MAX_SESSIONS],
    /// Session allocation bitmap (1 = allocated, 0 = free)
    allocated: [bool; MAX_SESSIONS],
    /// Currently active session ID
    active_id: usize,
    /// Which context is currently loaded in hardware (None = hardware not initialized)
    last_loaded: Option<usize>,
    /// Maximum number of sessions to support
    max_sessions: usize,
}

impl MultiContextProvider {
    /// Create a new multi-context provider
    ///
    /// # Arguments
    /// * `max_sessions` - Maximum concurrent sessions (must be <= `MAX_SESSIONS`)
    ///
    /// # Errors
    /// Returns `Err(SessionError)` if `max_sessions` > `MAX_SESSIONS` or `max_sessions` == 0
    pub fn new(max_sessions: usize) -> Result<Self, SessionError> {
        if max_sessions == 0 || max_sessions > MAX_SESSIONS {
            return Err(SessionError);
        }
        Ok(Self {
            contexts: [const { MaybeUninit::uninit() }; MAX_SESSIONS],
            allocated: [false; MAX_SESSIONS],
            active_id: 0,
            last_loaded: None,
            max_sessions,
        })
    }

    /// Allocate a new session slot
    ///
    /// Returns a session ID that can be used with `set_active_session()`.
    ///
    /// # Errors
    /// Returns `Err(SessionError)` if all session slots are allocated
    pub fn allocate_session(&mut self) -> Result<usize, SessionError> {
        for (id, allocated) in self
            .allocated
            .get_mut(..self.max_sessions)
            .ok_or(SessionError)?
            .iter_mut()
            .enumerate()
        {
            if !*allocated {
                *allocated = true;
                // Initialize the context with default values
                if let Some(ctx) = self.contexts.get_mut(id) {
                    *ctx = MaybeUninit::new(AspeedHashContext::default());
                    return Ok(id);
                }
            }
        }
        Err(SessionError)
    }

    /// Release a session slot
    ///
    /// # Arguments
    /// * `session_id` - Session ID returned by `allocate_session()`
    ///
    /// # Safety
    /// After releasing, the session ID must not be used again until reallocated.
    pub fn release_session(&mut self, session_id: usize) {
        if let Some(allocated) = self.allocated.get_mut(session_id) {
            if session_id < self.max_sessions && *allocated {
                *allocated = false;

                // Zero out the context for security using volatile writes to prevent optimization
                if let Some(ctx) = self.contexts.get_mut(session_id) {
                    // SAFETY: We're writing to allocated memory within bounds
                    unsafe {
                        let ctx_ptr = ctx.as_mut_ptr().cast::<u8>();
                        let size = core::mem::size_of::<AspeedHashContext>();
                        for i in 0..size {
                            core::ptr::write_volatile(ctx_ptr.add(i), 0);
                        }
                    }
                }

                // If this was the loaded context, invalidate the cache
                if self.last_loaded == Some(session_id) {
                    self.last_loaded = None;
                }
            }
        }
    }

    /// Set the active session for subsequent operations
    ///
    /// # Arguments
    /// * `session_id` - Session ID returned by `allocate_session()`
    ///
    /// # Panics
    /// Panics in debug builds if `session_id` is not allocated or out of bounds
    pub fn set_active_session(&mut self, session_id: usize) {
        debug_assert!(session_id < self.max_sessions, "Session ID out of bounds");
        debug_assert!(
            self.allocated.get(session_id).copied().unwrap_or(false),
            "Session ID not allocated: {session_id}"
        );
        self.active_id = session_id;
    }

    /// Get the currently active session ID
    #[must_use]
    pub const fn active_session(&self) -> usize {
        self.active_id
    }

    /// Check if a session is allocated
    #[must_use]
    pub fn is_session_allocated(&self, session_id: usize) -> bool {
        session_id < self.max_sessions && self.allocated.get(session_id).copied().unwrap_or(false)
    }

    /// Save hardware context to a storage slot
    ///
    /// # Errors
    /// Returns `ContextError` if:
    /// - `slot_id` is out of bounds
    /// - `slot_id` is not allocated
    fn save_hw_to_slot(
        &mut self,
        slot_id: usize,
    ) -> Result<(), crate::digest::traits::ContextError> {
        use crate::digest::traits::ContextError;

        // Runtime safety checks
        if slot_id >= self.max_sessions {
            return Err(ContextError::SessionOutOfBounds);
        }
        if !self.allocated.get(slot_id).copied().unwrap_or(false) {
            return Err(ContextError::SessionNotAllocated);
        }

        // SAFETY: We've verified slot_id is in bounds and allocated,
        // so the MaybeUninit is initialized
        let saved = unsafe {
            self.contexts
                .get_mut(slot_id)
                .ok_or(ContextError::SessionOutOfBounds)?
                .assume_init_mut()
        };

        // SAFETY: Single-threaded execution, no HACE interrupts enabled,
        // &mut self ensures exclusive access
        let hw_ctx = unsafe { &*crate::hace_controller::shared_hash_ctx() };

        // Copy all critical state from hardware context to storage
        saved.digest.copy_from_slice(&hw_ctx.digest);
        saved.digcnt = hw_ctx.digcnt;
        saved.bufcnt = hw_ctx.bufcnt;
        saved.buffer.copy_from_slice(&hw_ctx.buffer);
        saved.method = hw_ctx.method;
        saved.block_size = hw_ctx.block_size;
        saved.iv_size = hw_ctx.iv_size;

        // Save HMAC state
        saved.key.copy_from_slice(&hw_ctx.key);
        saved.key_len = hw_ctx.key_len;
        saved.ipad.copy_from_slice(&hw_ctx.ipad);
        saved.opad.copy_from_slice(&hw_ctx.opad);

        // Save scatter-gather descriptors
        saved.sg = hw_ctx.sg;

        Ok(())
    }

    /// Restore context from storage slot to hardware
    ///
    /// # Errors
    /// Returns `ContextError` if:
    /// - `slot_id` is out of bounds
    /// - `slot_id` is not allocated
    fn load_slot_to_hw(
        &mut self,
        slot_id: usize,
    ) -> Result<(), crate::digest::traits::ContextError> {
        use crate::digest::traits::ContextError;

        // Runtime safety checks
        if slot_id >= self.max_sessions {
            return Err(ContextError::SessionOutOfBounds);
        }
        if !self.allocated.get(slot_id).copied().unwrap_or(false) {
            return Err(ContextError::SessionNotAllocated);
        }

        // SAFETY: We've verified slot_id is in bounds and allocated,
        // so the MaybeUninit is initialized
        let saved = unsafe {
            self.contexts
                .get(slot_id)
                .ok_or(ContextError::SessionOutOfBounds)?
                .assume_init_ref()
        };

        // SAFETY: Single-threaded execution, no HACE interrupts enabled,
        // &mut self ensures exclusive access
        let hw_ctx = unsafe { &mut *crate::hace_controller::shared_hash_ctx() };

        // Copy all critical state from storage to hardware context
        hw_ctx.digest.copy_from_slice(&saved.digest);
        hw_ctx.digcnt = saved.digcnt;
        hw_ctx.bufcnt = saved.bufcnt;
        hw_ctx.buffer.copy_from_slice(&saved.buffer);
        hw_ctx.method = saved.method;
        hw_ctx.block_size = saved.block_size;
        hw_ctx.iv_size = saved.iv_size;

        // Restore HMAC state
        hw_ctx.key.copy_from_slice(&saved.key);
        hw_ctx.key_len = saved.key_len;
        hw_ctx.ipad.copy_from_slice(&saved.ipad);
        hw_ctx.opad.copy_from_slice(&saved.opad);

        // Restore scatter-gather descriptors
        hw_ctx.sg = saved.sg;

        Ok(())
    }
}

impl HaceContextProvider for MultiContextProvider {
    fn ctx_mut(&mut self) -> Result<&mut AspeedHashContext, crate::digest::traits::ContextError> {
        // Perform context switch if needed (lazy switching)
        if self.last_loaded != Some(self.active_id) {
            // Save previous context if one was loaded
            if let Some(prev_id) = self.last_loaded {
                // Invariant: prev_id was previously validated by set_active_session()
                // Error should never occur, but we ignore it to avoid panicking
                // TODO: Consider logging or debug assertion if error occurs
                let _ = self.save_hw_to_slot(prev_id);
            }
            // Invariant: active_id was validated by set_active_session()
            // Error should never occur, but we ignore it to avoid panicking
            // TODO: Consider logging or debug assertion if error occurs
            let _ = self.load_slot_to_hw(self.active_id);
            self.last_loaded = Some(self.active_id);
        }

        // SAFETY: Single-threaded execution, no HACE interrupts enabled,
        // &mut self ensures exclusive access to MultiContextProvider
        Ok(unsafe { &mut *crate::hace_controller::shared_hash_ctx() })
    }
}
