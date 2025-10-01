// Licensed under the Apache-2.0 license

//! Multi-session digest API for IPC servers
//!
//! This module provides a high-level, ergonomic API for managing multiple
//! concurrent hash sessions in IPC-based digest servers (e.g., Hubris).
//!
//! # Design
//!
//! The session API eliminates boilerplate by automatically handling:
//! - Session allocation and tracking
//! - Context switching between sessions
//! - Session validation and cleanup
//! - Type-safe algorithm verification
//!
//! # Examples
//!
//! ```no_run
//! use aspeed_ddk::digest::session::SessionManager;
//!
//! # fn example(hace: ast1060_pac::Hace) -> Result<(), aspeed_ddk::digest::session::SessionError> {
//! // Create manager supporting 8 concurrent sessions
//! let mut manager = SessionManager::<8>::new(hace)?;
//!
//! // Initialize a SHA-256 session
//! let mut session = manager.init_sha256()?;
//!
//! // Update with data (session auto-activated)
//! session = session.update(b"hello")?;
//! session = session.update(b" world")?;
//!
//! // Finalize and get digest
//! let (digest, _handle) = manager.finalize(session)?;
//! # Ok(())
//! # }
//! ```

use super::hace_controller::HaceController;
use super::hash_owned::{IntoHashAlgo, OwnedDigestContext, Sha2_256, Sha2_384, Sha2_512};
use super::multi_context::MultiContextProvider;
use ast1060_pac::Hace;
use core::marker::PhantomData;
use openprot_hal_blocking::digest::owned::{DigestInit, DigestOp};
use openprot_hal_blocking::digest::DigestAlgorithm;

/// Maximum recommended number of concurrent sessions
pub const MAX_SESSIONS: usize = 8;

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
    /// Invalid session count (N must be 1..=MAX_SESSIONS)
    InvalidSessionCount,
}

/// Algorithm type identifier
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlgorithmType {
    /// SHA-256
    Sha256,
    /// SHA-384
    Sha384,
    /// SHA-512
    Sha512,
}

/// Session slot state
#[derive(Clone, Copy, PartialEq, Eq)]
enum SlotState {
    /// Slot is free and available
    Free,
    /// Slot is allocated and in use
    Active,
}

/// Session slot metadata
#[derive(Clone, Copy)]
struct SessionSlot {
    /// Slot allocation state
    state: SlotState,
    /// Unique session ID (for validation)
    session_id: u32,
    /// Algorithm type (for debugging/validation)
    algorithm: Option<AlgorithmType>,
}

impl Default for SessionSlot {
    fn default() -> Self {
        Self {
            state: SlotState::Free,
            session_id: 0,
            algorithm: None,
        }
    }
}

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
/// use aspeed_ddk::digest::session::SessionManager;
///
/// # fn example(hace: ast1060_pac::Hace) -> Result<(), aspeed_ddk::digest::session::SessionError> {
/// let mut manager = SessionManager::<4>::new(hace)?;
///
/// // Start multiple sessions
/// let s1 = manager.init_sha256()?;
/// let s2 = manager.init_sha384()?;
///
/// // Update sessions (context switches automatically)
/// let s1 = s1.update(b"data1")?;
/// let s2 = s2.update(b"data2")?;
///
/// // Finalize in any order
/// let (digest1, _) = manager.finalize(s1)?;
/// let (digest2, _) = manager.finalize(s2)?;
/// # Ok(())
/// # }
/// ```
pub struct SessionManager<const N: usize> {
    /// The underlying controller (None when a session owns it)
    controller: Option<HaceController<MultiContextProvider>>,
    /// Session metadata (tracks which slot is used for what)
    sessions: [SessionSlot; N],
    /// Next session ID for uniqueness (wrapping counter)
    next_id: u32,
}

/// Opaque handle to a hash session
///
/// This handle is returned when finalizing a session and can be used
/// for validation or debugging. It cannot be used to access the session
/// after finalization.
///
/// # Type Safety
///
/// The handle is generic over the digest algorithm, preventing
/// accidental algorithm mismatches at compile time.
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
/// 3. Finalized via `SessionManager::finalize()`
///
/// # Drop Behavior
///
/// This type does NOT implement `Drop`. Sessions must be explicitly
/// finalized via `SessionManager::finalize()` or canceled via
/// `SessionManager::cancel()` to prevent resource leaks.
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

impl<T: DigestAlgorithm + IntoHashAlgo> SessionDigest<T>
where
    OwnedDigestContext<T, MultiContextProvider>: DigestOp<Output = T::Digest>,
{
    /// Update the digest with additional data
    ///
    /// The session is automatically activated before the update operation.
    /// This consumes `self` and returns a new instance (move semantics).
    ///
    /// # Errors
    ///
    /// Returns `SessionError::UpdateFailed` if the update operation fails.
    pub fn update(mut self, data: &[u8]) -> Result<Self, SessionError> {
        // Activate session in provider
        self.context
            .controller_mut()
            .provider_mut()
            .set_active_session(self.provider_session_id);

        // Perform update using DigestOp trait
        self.context =
            DigestOp::update(self.context, data).map_err(|_| SessionError::UpdateFailed)?;

        Ok(self)
    }

    /// Get session handle for this digest
    ///
    /// The handle can be used for validation but cannot be used to
    /// access the session after it has been moved.
    #[must_use]
    pub fn handle(&self) -> SessionHandle<T> {
        SessionHandle {
            slot: self.slot,
            id: self.manager_session_id,
            _marker: PhantomData,
        }
    }

    /// Get the provider session ID
    ///
    /// This is primarily for debugging and internal use.
    #[must_use]
    pub const fn provider_session_id(&self) -> usize {
        self.provider_session_id
    }

    /// Get the manager session ID
    ///
    /// This is primarily for debugging and internal use.
    #[must_use]
    pub const fn manager_session_id(&self) -> u32 {
        self.manager_session_id
    }
}

impl<const N: usize> SessionManager<N> {
    /// Create a new session manager
    ///
    /// # Errors
    ///
    /// Returns `SessionError::InvalidSessionCount` if N is 0 or greater than MAX_SESSIONS.
    pub fn new(hace: Hace) -> Result<Self, SessionError> {
        if N == 0 || N > MAX_SESSIONS {
            return Err(SessionError::InvalidSessionCount);
        }

        let provider =
            MultiContextProvider::new(N).map_err(|_| SessionError::InvalidSessionCount)?;

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
    ///
    /// Returns `SessionError::TooManySessions` if all session slots are full.
    pub fn init_sha256(&mut self) -> Result<SessionDigest<Sha2_256>, SessionError> {
        self.init_session::<Sha2_256>(AlgorithmType::Sha256, Sha2_256)
    }

    /// Initialize a new SHA-384 session
    ///
    /// # Errors
    ///
    /// Returns `SessionError::TooManySessions` if all session slots are full.
    pub fn init_sha384(&mut self) -> Result<SessionDigest<Sha2_384>, SessionError> {
        self.init_session::<Sha2_384>(AlgorithmType::Sha384, Sha2_384)
    }

    /// Initialize a new SHA-512 session
    ///
    /// # Errors
    ///
    /// Returns `SessionError::TooManySessions` if all session slots are full.
    pub fn init_sha512(&mut self) -> Result<SessionDigest<Sha2_512>, SessionError> {
        self.init_session::<Sha2_512>(AlgorithmType::Sha512, Sha2_512)
    }

    /// Generic session initialization
    fn init_session<T>(
        &mut self,
        algo: AlgorithmType,
        init_params: T,
    ) -> Result<SessionDigest<T>, SessionError>
    where
        T: DigestAlgorithm + IntoHashAlgo,
        HaceController<MultiContextProvider>:
            DigestInit<T, Context = OwnedDigestContext<T, MultiContextProvider>>,
    {
        // Find free slot
        let slot = self
            .sessions
            .iter()
            .position(|s| s.state == SlotState::Free)
            .ok_or(SessionError::TooManySessions)?;

        // Take controller
        let mut controller = self
            .controller
            .take()
            .ok_or(SessionError::ControllerInUse)?;

        // Allocate provider session
        let provider_session_id = controller
            .provider_mut()
            .allocate_session()
            .map_err(|_| SessionError::TooManySessions)?;

        // Set as active
        controller
            .provider_mut()
            .set_active_session(provider_session_id);

        // Initialize digest using DigestInit trait
        let context = DigestInit::init(controller, init_params)
            .map_err(|_| SessionError::InitializationFailed)?;

        // Generate unique session ID
        let manager_session_id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        // Mark slot as active
        if let Some(slot_data) = self.sessions.get_mut(slot) {
            *slot_data = SessionSlot {
                state: SlotState::Active,
                session_id: manager_session_id,
                algorithm: Some(algo),
            };
        }

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
    /// # Errors
    ///
    /// Returns error if session is invalid or finalization fails.
    pub fn finalize<T>(
        &mut self,
        digest: SessionDigest<T>,
    ) -> Result<(T::Digest, SessionHandle<T>), SessionError>
    where
        T: DigestAlgorithm + IntoHashAlgo,
        OwnedDigestContext<T, MultiContextProvider>:
            DigestOp<Output = T::Digest, Controller = HaceController<MultiContextProvider>>,
    {
        // Validate session
        let slot_data = self
            .sessions
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

        // Finalize digest using DigestOp trait
        let (output, mut controller) =
            DigestOp::finalize(context).map_err(|_| SessionError::FinalizationFailed)?;

        // Release provider session
        controller
            .provider_mut()
            .release_session(digest.provider_session_id);

        // Mark slot as free
        if let Some(slot_data) = self.sessions.get_mut(digest.slot) {
            *slot_data = SessionSlot {
                state: SlotState::Free,
                session_id: 0,
                algorithm: None,
            };
        }

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
    ///
    /// # Errors
    ///
    /// Returns `SessionError::InvalidSession` if the session ID is invalid.
    pub fn cancel<T>(&mut self, digest: SessionDigest<T>) -> Result<(), SessionError>
    where
        T: DigestAlgorithm + IntoHashAlgo,
    {
        // Validate session
        let slot_data = self
            .sessions
            .get(digest.slot)
            .ok_or(SessionError::InvalidSession)?;

        if slot_data.session_id != digest.manager_session_id {
            return Err(SessionError::InvalidSession);
        }

        // Cancel context
        let mut controller = digest.context.cancel();

        // Release provider session
        controller
            .provider_mut()
            .release_session(digest.provider_session_id);

        // Mark slot as free
        if let Some(slot_data) = self.sessions.get_mut(digest.slot) {
            *slot_data = SessionSlot {
                state: SlotState::Free,
                session_id: 0,
                algorithm: None,
            };
        }

        // Return controller
        self.controller = Some(controller);

        Ok(())
    }

    /// Get the number of active sessions
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.sessions
            .iter()
            .filter(|s| s.state == SlotState::Active)
            .count()
    }

    /// Check if a session is valid
    #[must_use]
    pub fn is_valid<T>(&self, handle: &SessionHandle<T>) -> bool {
        self.sessions.get(handle.slot).map_or(false, |s| {
            s.session_id == handle.id && s.state == SlotState::Active
        })
    }

    /// Get the maximum number of sessions supported
    #[must_use]
    pub const fn max_sessions(&self) -> usize {
        N
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests verify compilation and API structure.
    // Integration tests with actual hardware would require HACE peripheral.

    #[test]
    fn test_api_structure() {
        // Verify types compile and have expected signatures
        fn _check_manager_is_send_sync()
        where
            SessionManager<4>: Send + Sync,
        {
        }

        fn _check_handle_is_send_sync<T>()
        where
            SessionHandle<T>: Send + Sync,
        {
        }
    }

    #[test]
    fn test_session_manager_properties() {
        // Test const functions work in const context
        const _MAX: usize = MAX_SESSIONS;

        // Verify session count validation
        type TooMany = SessionManager<{ MAX_SESSIONS + 1 }>;
        type Valid = SessionManager<4>;
    }
}
