// Licensed under the Apache-2.0 license

#![cfg_attr(not(test), no_std)]
pub mod astdebug;
pub mod common;
pub mod digest;
pub mod ecdsa;
pub mod gpio;
pub mod i2c;
pub mod pinctrl;
pub mod rsa;
pub mod spi;
pub mod spimonitor;
pub mod syscon;
pub mod tests;
pub mod timer;
pub mod uart;
pub mod watchdog;
