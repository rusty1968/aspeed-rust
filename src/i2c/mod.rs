// Licensed under the Apache-2.0 license

//! ASPEED I2C driver module.
//!
//! This module provides I2C controller and device implementations for ASPEED `SoCs`,
//! specifically designed for bare-metal and `no_std` environments. It integrates
//! hardware-specific implementations with high-level abstractions for I2C communication.

// Licensed under the Apache-2.0 license

pub mod ast1060_i2c;
pub mod common;
pub mod i2c_controller;
