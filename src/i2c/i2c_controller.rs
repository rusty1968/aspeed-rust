// Licensed under the Apache-2.0 license

//! High-level I2C controller abstraction for ASPEED `SoCs`.
//!
//! This module provides safe APIs for configuring, sending, and receiving I2C transactions.
//! It implements embedded-hal compatible interfaces and is designed for use in `no_std`
//! environments with hardware abstraction traits.

// Licensed under the Apache-2.0 license

use crate::common::{Logger, NoOpLogger};
use crate::i2c::common::I2cConfig;
use crate::i2c::traits::I2cMaster;
use embedded_hal::i2c::{Operation, SevenBitAddress};

pub struct I2cController<H: I2cMaster, L: Logger = NoOpLogger> {
    pub hardware: H,
    pub config: I2cConfig,
    pub logger: L,
}

impl<H: I2cMaster, L: Logger> embedded_hal::i2c::ErrorType for I2cController<H, L> {
    type Error = H::Error;
}

impl<H: I2cMaster, L: Logger> embedded_hal::i2c::I2c for I2cController<H, L> {
    fn read(&mut self, addr: SevenBitAddress, buffer: &mut [u8]) -> Result<(), Self::Error> {
        self.hardware.read(addr, buffer)
    }

    fn write(&mut self, addr: SevenBitAddress, bytes: &[u8]) -> Result<(), Self::Error> {
        self.hardware.write(addr, bytes)
    }

    fn write_read(
        &mut self,
        addr: SevenBitAddress,
        bytes: &[u8],
        buffer: &mut [u8],
    ) -> Result<(), Self::Error> {
        self.hardware.write_read(addr, bytes, buffer)
    }

    fn transaction(
        &mut self,
        addr: SevenBitAddress,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        self.hardware.transaction_slice(addr, operations)
    }
}
