// Licensed under the Apache-2.0 license

use crate::hace_controller::AspeedHashContext;

/// Trait abstracting how hash context is accessed
pub trait HaceContextProvider {
    /// Get mutable reference to the active hash context
    fn ctx_mut(&mut self) -> &mut AspeedHashContext;
}
