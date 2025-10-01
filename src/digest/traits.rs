// Licensed under the Apache-2.0 license

use super::hace_controller::AspeedHashContext;

/// Error type for context provider operations
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ContextError {
    /// Session ID is out of bounds
    SessionOutOfBounds,
    /// Session is not allocated
    SessionNotAllocated,
    /// Internal context switching error
    ContextSwitchFailed,
}

/// Trait abstracting how hash context is accessed
pub trait HaceContextProvider {
    /// Get mutable reference to the active hash context
    ///
    /// # Errors
    /// Returns `ContextError` if context access fails
    fn ctx_mut(&mut self) -> Result<&mut AspeedHashContext, ContextError>;
}

/// Single-context provider that uses the global shared context (zero overhead)
///
/// This is the default provider for `HaceController` and provides the same
/// behavior as the original non-generic implementation.
pub struct SingleContextProvider;

impl HaceContextProvider for SingleContextProvider {
    fn ctx_mut(&mut self) -> Result<&mut AspeedHashContext, ContextError> {
        // SAFETY: Single-threaded execution, no HACE interrupts enabled
        Ok(unsafe { &mut *super::hace_controller::shared_hash_ctx() })
    }
}

// Re-export MultiContextProvider when the feature is enabled
#[cfg(feature = "multi-context")]
pub use crate::digest::multi_context::MultiContextProvider;
