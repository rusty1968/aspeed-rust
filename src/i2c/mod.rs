// Licensed under the Apache-2.0 license

//! ASPEED I2C driver module.
//!
//! This module provides comprehensive I2C controller and device implementations for ASPEED SoCs,
//! specifically designed for bare-metal and `no_std` environments. It integrates hardware-specific
//! implementations with standardized high-level abstractions for I2C communication.
//!
//! ## Features
//!
//! - **Hardware-specific implementation**: Direct AST1060 I2C controller support
//! - **OpenProt ecosystem compatibility**: Implements OpenProt HAL traits for standardized APIs
//! - **Master and slave mode support**: Complete I2C functionality with feature gating
//! - **Multiple transfer modes**: DMA, buffer, and byte-mode operations
//! - **No-std compatibility**: Designed for bare-metal embedded environments
//!
//! ## OpenProt Integration
//!
//! This module implements the OpenProt HAL blocking I2C traits, providing:
//!
//! ### Master Mode Traits
//! - `I2cHardwareCore`: Core hardware abstraction 
//! - `I2cMaster`: Master mode I2C operations
//!
//! ### Slave Mode Traits (feature: `i2c_target`)
//! - `I2cSlaveCore`: Basic slave configuration and mode control
//! - `I2cSlaveInterrupts`: Interrupt and status management 
//! - `I2cSlaveBuffer`: Buffer operations for data transfer
//! - `I2cSlaveEventSync`: Blocking event handling and synchronization
//! - `I2cSlaveSync`: Composite trait combining core + buffer + event sync (automatic)
//! - `I2cMasterSlave`: Complete controller supporting both modes (automatic)
//!
//! ## Usage Example
//!
//! ```rust,no_run
//! use aspeed_ddk::i2c::ast1060_i2c::Ast1060I2c;
//! use openprot_hal_blocking::i2c_hardware::{I2cHardwareCore, I2cMaster};
//! 
//! // Master mode usage
//! let mut i2c = Ast1060I2c::new(/* ... */);
//! i2c.init(400_000)?; // 400kHz
//! 
//! // Read from slave device
//! let mut buffer = [0u8; 4];
//! i2c.read(0x50, &mut buffer)?;
//! ```
//!
//! ```rust,no_run
//! #[cfg(feature = "i2c_target")]
//! use openprot_hal_blocking::i2c_hardware::slave::{I2cSlaveCore, I2cMasterSlave};
//! 
//! // Slave mode usage (requires i2c_target feature)
//! let mut i2c = Ast1060I2c::new(/* ... */);
//! i2c.configure_slave_address(0x42)?;
//! i2c.enable_slave_mode()?;
//! ```
//!
//! ## Module Organization
//!
//! - `ast1060_i2c`: Hardware-specific AST1060 I2C controller implementation
//! - `openprot_slave_impl`: OpenProt slave trait implementations  
//! - `common`: Shared types and utilities
//! - `i2c_controller`: Higher-level controller abstractions

// Licensed under the Apache-2.0 license

pub mod ast1060_i2c;
pub mod common;
pub mod i2c_controller;
pub mod openprot_slave_impl;
