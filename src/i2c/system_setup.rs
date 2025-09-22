// Licensed under the Apache-2.0 license

//! I2C System Setup Helper
//!
//! This module provides helper functions for I2C system control operations,
//! enabling clean separation between I2C hardware control and system-level
//! configuration through `OpenProt` `SystemControl` traits.

use crate::i2c::ast1060_i2c::Error;
use crate::syscon::{ClockId, ResetId};
use openprot_hal_blocking::system_control::{ErrorType, SystemControl};

/// Helper for I2C system control operations using existing `SysCon` infrastructure
pub struct I2cSystemSetup;

impl I2cSystemSetup {
    /// Complete I2C system initialization using `SystemControl`
    ///
    /// Performs all operations that were previously hardcoded in `init()`:
    /// - I2C/SMBus controller reset
    /// - Clock enabling and configuration
    /// - System-level I2C setup
    ///
    /// # Arguments
    ///
    /// * `system_controller` - Mutable reference to `SystemControl` implementation
    ///
    /// # Returns
    ///
    /// * `Result<(), Error>` - Ok if initialization succeeds, error otherwise
    pub fn initialize_i2c_system<S>(system_controller: &mut S) -> Result<(), Error>
    where
        S: SystemControl<ClockId = ClockId, ResetId = ResetId>,
        Error: From<<S as ErrorType>::Error>,
    {
        // Reset I2C/SMBus controller (replaces: scu.scu050().write())
        system_controller
            .reset_assert(&ResetId::RstI2C)
            .map_err(Error::from)?;

        // Clear reset and configure (replaces: scu.scu054().write())
        system_controller
            .reset_deassert(&ResetId::RstI2C)
            .map_err(Error::from)?;

        // Enable I2C clocks
        system_controller
            .enable(&ClockId::ClkPCLK)
            .map_err(Error::from)?;

        Ok(())
    }

    /// Configure I2C clocks for optimal performance
    ///
    /// Sets the APB clock frequency which is used as the I2C source clock.
    /// This replaces hardcoded frequency assumptions.
    ///
    /// # Arguments
    ///
    /// * `system_controller` - Mutable reference to `SystemControl` implementation
    /// * `target_frequency` - Desired frequency in Hz
    ///
    /// # Returns
    ///
    /// * `Result<(), Error>` - Ok if configuration succeeds, error otherwise
    pub fn configure_i2c_clocks<S>(
        system_controller: &mut S,
        target_frequency: u64,
    ) -> Result<(), Error>
    where
        S: SystemControl<ClockId = ClockId, ResetId = ResetId>,
        Error: From<<S as ErrorType>::Error>,
    {
        // Configure APB clock frequency (replaces hardcoded HPLL calculations)
        system_controller
            .set_frequency(&ClockId::ClkPCLK, target_frequency)
            .map_err(Error::from)?;

        Ok(())
    }

    /// Get I2C source clock frequency for timing calculations
    ///
    /// Retrieves the current APB clock frequency which serves as the I2C
    /// source clock. This replaces direct SCU register reads.
    ///
    /// # Arguments
    ///
    /// * `system_controller` - Reference to `SystemControl` implementation
    ///
    /// # Returns
    ///
    /// * `Result<u64, Error>` - Clock frequency in Hz, or error
    pub fn get_i2c_source_frequency<S>(system_controller: &S) -> Result<u64, Error>
    where
        S: SystemControl<ClockId = ClockId, ResetId = ResetId>,
        Error: From<<S as ErrorType>::Error>,
    {
        // Get PCLK frequency (replaces: hardcoded SCU calculation)
        system_controller
            .get_frequency(&ClockId::ClkPCLK)
            .map_err(Error::from)
    }

    /// Perform complete I2C initialization with clock configuration
    ///
    /// This is a convenience method that combines system initialization
    /// and clock configuration in one call.
    ///
    /// # Arguments
    ///
    /// * `system_controller` - Mutable reference to `SystemControl` implementation
    /// * `clock_frequency` - Desired I2C source clock frequency in Hz
    ///
    /// # Returns
    ///
    /// * `Result<u64, Error>` - Actual configured frequency, or error
    pub fn initialize_with_clock_config<S>(
        system_controller: &mut S,
        clock_frequency: u64,
    ) -> Result<u64, Error>
    where
        S: SystemControl<ClockId = ClockId, ResetId = ResetId>,
        Error: From<<S as ErrorType>::Error>,
    {
        // Configure clocks first
        Self::configure_i2c_clocks(system_controller, clock_frequency)?;

        // Perform system initialization
        Self::initialize_i2c_system(system_controller)?;

        // Return actual configured frequency
        Self::get_i2c_source_frequency(system_controller)
    }

    /// Reset I2C peripheral only (without full system initialization)
    ///
    /// This method performs just the reset operation, useful for
    /// error recovery or partial reinitialization.
    ///
    /// # Arguments
    ///
    /// * `system_controller` - Mutable reference to `SystemControl` implementation
    ///
    /// # Returns
    ///
    /// * `Result<(), Error>` - Ok if reset succeeds, error otherwise
    pub fn reset_i2c_peripheral<S>(system_controller: &mut S) -> Result<(), Error>
    where
        S: SystemControl<ClockId = ClockId, ResetId = ResetId>,
        Error: From<<S as ErrorType>::Error>,
    {
        // Assert reset
        system_controller
            .reset_assert(&ResetId::RstI2C)
            .map_err(Error::from)?;

        // Deassert reset
        system_controller
            .reset_deassert(&ResetId::RstI2C)
            .map_err(Error::from)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    struct MockSystemController {
        clock_frequencies: HashMap<ClockId, u64>,
        enabled_clocks: HashSet<ClockId>,
        reset_states: HashMap<ResetId, bool>,
    }

    impl MockSystemController {
        fn new() -> Self {
            Self {
                clock_frequencies: HashMap::new(),
                enabled_clocks: HashSet::new(),
                reset_states: HashMap::new(),
            }
        }
    }

    impl ErrorType for MockSystemController {
        type Error = ();
    }

    impl SystemControl for MockSystemController {
        type ClockId = ClockId;
        type ResetId = ResetId;
        type ClockConfig = crate::syscon::ClockConfig;

        fn enable(&mut self, clock_id: &Self::ClockId) -> Result<(), Self::Error> {
            self.enabled_clocks.insert(*clock_id);
            Ok(())
        }

        fn disable(&mut self, clock_id: &Self::ClockId) -> Result<(), Self::Error> {
            self.enabled_clocks.remove(clock_id);
            Ok(())
        }

        fn set_frequency(
            &mut self,
            clock_id: &Self::ClockId,
            frequency: u64,
        ) -> Result<(), Self::Error> {
            self.clock_frequencies.insert(*clock_id, frequency);
            Ok(())
        }

        fn get_frequency(&self, clock_id: &Self::ClockId) -> Result<u64, Self::Error> {
            self.clock_frequencies.get(clock_id).copied().ok_or(())
        }

        fn reset_assert(&mut self, reset_id: &Self::ResetId) -> Result<(), Self::Error> {
            self.reset_states.insert(*reset_id, true);
            Ok(())
        }

        fn reset_deassert(&mut self, reset_id: &Self::ResetId) -> Result<(), Self::Error> {
            self.reset_states.insert(*reset_id, false);
            Ok(())
        }

        fn reset_pulse(
            &mut self,
            reset_id: &Self::ResetId,
            _duration_us: u32,
        ) -> Result<(), Self::Error> {
            self.reset_assert(reset_id)?;
            self.reset_deassert(reset_id)?;
            Ok(())
        }

        fn configure_clock(
            &mut self,
            _clock_id: &Self::ClockId,
            _config: &Self::ClockConfig,
        ) -> Result<(), Self::Error> {
            Ok(())
        }

        fn get_clock_config(
            &self,
            _clock_id: &Self::ClockId,
        ) -> Result<Self::ClockConfig, Self::Error> {
            Err(())
        }
    }

    #[test]
    fn test_initialize_i2c_system() {
        let mut mock = MockSystemController::new();

        let result = I2cSystemSetup::initialize_i2c_system(&mut mock);

        assert!(result.is_ok());
        assert!(mock.enabled_clocks.contains(&ClockId::ClkPCLK));
        assert_eq!(mock.reset_states.get(&ResetId::RstI2C), Some(&false));
    }

    #[test]
    fn test_configure_i2c_clocks() {
        let mut mock = MockSystemController::new();
        let target_freq = 50_000_000;

        let result = I2cSystemSetup::configure_i2c_clocks(&mut mock, target_freq);

        assert!(result.is_ok());
        assert_eq!(
            mock.clock_frequencies.get(&ClockId::ClkPCLK),
            Some(&target_freq)
        );
    }

    #[test]
    fn test_get_i2c_source_frequency() {
        let mut mock = MockSystemController::new();
        let expected_freq = 48_000_000;
        mock.clock_frequencies
            .insert(ClockId::ClkPCLK, expected_freq);

        let result = I2cSystemSetup::get_i2c_source_frequency(&mock);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), expected_freq);
    }

    #[test]
    fn test_initialize_with_clock_config() {
        let mut mock = MockSystemController::new();
        let clock_freq = 50_000_000;

        let result = I2cSystemSetup::initialize_with_clock_config(&mut mock, clock_freq);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), clock_freq);
        assert!(mock.enabled_clocks.contains(&ClockId::ClkPCLK));
        assert_eq!(mock.reset_states.get(&ResetId::RstI2C), Some(&false));
    }
}
