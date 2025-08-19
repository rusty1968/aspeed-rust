// Licensed under the Apache-2.0 license

pub mod hardware;
pub mod digest_impl;
pub mod hmac_impl;

// Re-export key types for convenience
pub use hardware::{
    AspeedHashContext, AspeedSg, ContextCleanup, HaceController, 
    HashAlgo, HardwareError, HACE_SG_EN, HACE_SG_LAST
};

// Re-export digest bridge types
pub use digest_impl::HashContext;
