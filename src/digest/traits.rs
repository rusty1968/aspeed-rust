// Licensed under the Apache-2.0 license

use crate::hace_controller::AspeedHashContext;

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
