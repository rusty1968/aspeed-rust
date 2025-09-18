// Licensed under the Apache-2.0 license

//! I2C module for ASPEED DDK
//! Provides I2C master and optional slave mode functionality

pub mod common;
pub mod hardware_interface;

// Core I2C implementation (always available)
pub mod ast1060;

// Slave mode implementation (only when feature enabled)
#[cfg(feature = "i2c_target")]
pub mod ast1060_slave_impl;

// Re-export common types for convenience
pub use common::{
    I2cConfig, I2cConfigBuilder, I2cSpeed, I2cXferMode, 
    I2cSEvent, TimingConfig, ConfigurationError
};

// Re-export slave types when available
#[cfg(feature = "i2c_target")]
pub use common::{SlaveStatus, SlaveMessage, SlaveConfig, SlaveMessageError};

// Re-export hardware interfaces
pub use hardware_interface::HardwareInterface;

#[cfg(feature = "i2c_target")]
pub use hardware_interface::SlaveHardwareInterface;

// Re-export main I2C implementation
pub use ast1060::Ast1060I2c;
