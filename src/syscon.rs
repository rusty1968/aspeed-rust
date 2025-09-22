// Licensed under the Apache-2.0 license

use ast1060_pac::Scu;
use core::time::Duration;
use embedded_hal::delay::DelayNs;
use openprot_hal_blocking::system_control::{
    ClockControl, Error as SystemControlError, ErrorKind, ErrorType, ResetControl,
};

const ASPEED_CLK_GRP_0_OFFSET: u8 = 0;
const ASPEED_CLK_GRP_1_OFFSET: u8 = 32;
const ASPEED_CLK_GRP_2_OFFSET: u8 = 64; //dummy

const ASPEED_RESET_GRP_0_OFFSET: u8 = 0;
const ASPEED_RESET_GRP_1_OFFSET: u8 = 32;
const ASPEED_I3C_CLOCK_DIVIDER_MAX: u8 = 7;
const ASPEED_HCLK_CLOCK_DIVIDER_MAX: u8 = 7;
const ASPEED_PCLK_CLOCK_DIVIDER_MAX: u8 = 15;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ClockId {
    ClkMCLK = ASPEED_CLK_GRP_0_OFFSET,
    ClkYCLK = (ASPEED_CLK_GRP_0_OFFSET + 13),
    ClkREFCLK = (ASPEED_CLK_GRP_1_OFFSET + 2),
    ClkRSACLK = (ASPEED_CLK_GRP_1_OFFSET + 6),
    ClkI3C0 = (ASPEED_CLK_GRP_1_OFFSET + 8),
    ClkI3C1 = (ASPEED_CLK_GRP_1_OFFSET + 9),
    ClkI3C2 = (ASPEED_CLK_GRP_1_OFFSET + 10),
    ClkI3C3 = (ASPEED_CLK_GRP_1_OFFSET + 11),
    ClkPCLK = ASPEED_CLK_GRP_2_OFFSET,
    ClkHCLK = (ASPEED_CLK_GRP_2_OFFSET + 1),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ResetId {
    RstHACE = (ASPEED_RESET_GRP_0_OFFSET + 4),
    RstSRAM = ASPEED_RESET_GRP_0_OFFSET,
    RstUART4 = (ASPEED_RESET_GRP_1_OFFSET + 31),
    RstUART3 = (ASPEED_RESET_GRP_1_OFFSET + 30),
    RstUART2 = (ASPEED_RESET_GRP_1_OFFSET + 29),
    RstUART1 = (ASPEED_RESET_GRP_1_OFFSET + 28),
    RstJTAGM0 = (ASPEED_RESET_GRP_1_OFFSET + 26),
    RstADC = (ASPEED_RESET_GRP_1_OFFSET + 23),
    RstJTAGM1 = (ASPEED_RESET_GRP_1_OFFSET + 22),
    RstI3C3 = (ASPEED_RESET_GRP_1_OFFSET + 11),
    RstI3C2 = (ASPEED_RESET_GRP_1_OFFSET + 10),
    RstI3C1 = (ASPEED_RESET_GRP_1_OFFSET + 9),
    RstI3C0 = (ASPEED_RESET_GRP_1_OFFSET + 8),
    RstI3C = (ASPEED_RESET_GRP_1_OFFSET + 7),
    RstI2C = (ASPEED_RESET_GRP_1_OFFSET + 2),
}

const fn mhz(x: u32) -> u32 {
    x * 1_000_000
}

const I3C_CLK_SRC_480MHZ: bool = true;
const HPLL_FREQ: u32 = mhz(1000); //1000Mhz

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum I3CClkSource {
    I3CHPLL = 0,
    I3C480MHZ = 1,
}

pub enum HCLKSource {
    HPLL4 = 0,
    HPLL2 = 1,
    HPLL = 2,
    HCLK = 3,
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[non_exhaustive]
pub enum Error {
    ClockNotFound,
    ClockAlreadyEnabled,
    ClockAlreadyDisabled,
    InvalidClockFrequency,
    ClockConfigurationFailed,
    InvalidResetId,
    HardwareFailure,
    PermissionDenied,
    Timeout,
    InvalidClkSource,
}

pub struct SysCon<D: DelayNs> {
    delay: D,
    scu: Scu,
}

impl SystemControlError for Error {
    fn kind(&self) -> ErrorKind {
        match *self {
            Self::ClockNotFound => ErrorKind::ClockNotFound,
            Self::ClockAlreadyEnabled => ErrorKind::ClockAlreadyEnabled,
            Self::ClockAlreadyDisabled => ErrorKind::ClockAlreadyDisabled,
            Self::InvalidClockFrequency => ErrorKind::InvalidClockFrequency,
            Self::ClockConfigurationFailed => ErrorKind::ClockConfigurationFailed,
            Self::InvalidResetId => ErrorKind::InvalidResetId,
            Self::HardwareFailure => ErrorKind::HardwareFailure,
            Self::PermissionDenied | Self::InvalidClkSource => ErrorKind::PermissionDenied,
            Self::Timeout => ErrorKind::Timeout,
        }
    }
}

impl<D: DelayNs> ErrorType for SysCon<D> {
    type Error = Error;
}

impl<D: DelayNs> SysCon<D> {
    pub fn new(delay: D, scu: Scu) -> Self {
        Self { delay, scu }
    }
    /// Clock Stop Control Clear
    /// `clock_bit`: clock enable bit position
    ///
    pub fn enable_clock(&mut self, clock_bit: u8) -> Result<(), Error> {
        let mut bit_pos = clock_bit;
        if bit_pos >= ASPEED_CLK_GRP_2_OFFSET {
            return Ok(());
        }
        if bit_pos >= ASPEED_CLK_GRP_1_OFFSET {
            bit_pos -= ASPEED_CLK_GRP_1_OFFSET;
            if self.scu.scu090().read().bits() & (1 << bit_pos) == (1 << bit_pos) {
                self.scu.scu094().write(|w| unsafe { w.bits(1 << bit_pos) });
            } else {
                return Err(Error::ClockAlreadyEnabled);
            }
        } else if self.scu.scu080().read().bits() & (1 << bit_pos) == (1 << bit_pos) {
            self.scu.scu084().write(|w| unsafe { w.bits(1 << bit_pos) });
        } else {
            return Err(Error::ClockAlreadyEnabled);
        }
        Ok(())
    }

    pub fn disable_clock(&mut self, clock_bit: u8) -> Result<(), Error> {
        let mut bit_pos = clock_bit;
        if bit_pos >= ASPEED_CLK_GRP_2_OFFSET {
            return Ok(());
        }
        if bit_pos >= ASPEED_CLK_GRP_1_OFFSET {
            bit_pos -= ASPEED_CLK_GRP_1_OFFSET;
            if self.scu.scu090().read().bits() & (1 << bit_pos) == (1 << bit_pos) {
                return Err(Error::ClockAlreadyDisabled);
            }
            self.scu.scu090().write(|w| unsafe { w.bits(1 << bit_pos) });
        } else if self.scu.scu080().read().bits() & (1 << bit_pos) == (1 << bit_pos) {
            return Err(Error::ClockAlreadyDisabled);
        } else {
            self.scu.scu080().write(|w| unsafe { w.bits(1 << bit_pos) });
        }
        Ok(())
    }

    fn set_frequency(&mut self, clock_id: ClockId, frequency_hz: u64) -> Result<(), Error> {
        let src: u32;
        let clk_div: u32;
        let freq = u32::try_from(frequency_hz).map_err(|_| Error::InvalidClockFrequency)?;
        match clock_id {
            ClockId::ClkI3C0 | ClockId::ClkI3C1 | ClockId::ClkI3C2 | ClockId::ClkI3C3 => {
                if self.scu.scu310().read().i3cclk_source_sel().bit() == I3C_CLK_SRC_480MHZ {
                    src = mhz(480);
                } else {
                    src = HPLL_FREQ;
                }
                clk_div = src / freq;
                if clk_div <= u32::from(ASPEED_I3C_CLOCK_DIVIDER_MAX) {
                    let divider =
                        u8::try_from(clk_div).map_err(|_| Error::InvalidClockFrequency)?;
                    self.scu
                        .scu310()
                        .modify(|_, w| unsafe { w.i3cclk_divider_sel().bits(divider) });
                    Ok(())
                } else {
                    Err(Error::InvalidClockFrequency)
                }
            }
            ClockId::ClkHCLK => {
                src = HPLL_FREQ;
                clk_div = src / freq;
                if clk_div <= u32::from(ASPEED_HCLK_CLOCK_DIVIDER_MAX) {
                    let divider =
                        u8::try_from(clk_div).map_err(|_| Error::InvalidClockFrequency)?;
                    self.scu
                        .scu314()
                        .modify(|_, w| unsafe { w.hclkdivider_sel().bits(divider) });
                    Ok(())
                } else {
                    Err(Error::InvalidClockFrequency)
                }
            }
            ClockId::ClkPCLK => {
                src = HPLL_FREQ;
                clk_div = src / freq;
                if clk_div <= u32::from(ASPEED_PCLK_CLOCK_DIVIDER_MAX) {
                    let divider =
                        u8::try_from(clk_div).map_err(|_| Error::InvalidClockFrequency)?;
                    self.scu
                        .scu310()
                        .modify(|_, w| unsafe { w.apbbus_pclkdivider_sel().bits(divider) });
                    Ok(())
                } else {
                    Err(Error::InvalidClockFrequency)
                }
            }
            _ => Err(Error::PermissionDenied),
        }
    }

    fn get_frequency(&self, clock_id: ClockId) -> Result<u64, Error> {
        let src: u32;
        let clk_div: u32;
        let freq: u32;
        match clock_id {
            ClockId::ClkI3C0 | ClockId::ClkI3C1 | ClockId::ClkI3C2 | ClockId::ClkI3C3 => {
                if self.scu.scu310().read().i3cclk_source_sel().bit() == I3C_CLK_SRC_480MHZ {
                    src = mhz(480);
                } else {
                    src = HPLL_FREQ;
                }
                clk_div = u32::from(self.scu.scu310().read().i3cclk_divider_sel().bits());
                freq = src / clk_div;
            }
            ClockId::ClkHCLK => {
                src = HPLL_FREQ;
                clk_div = u32::from(self.scu.scu314().read().hclkdivider_sel().bits());
                freq = src / clk_div;
            }
            ClockId::ClkPCLK => {
                src = HPLL_FREQ;
                clk_div = u32::from(self.scu.scu310().read().apbbus_pclkdivider_sel().bits());
                freq = src / clk_div;
            }
            _ => {
                freq = 0;
            }
        }
        if freq == 0 {
            return Err(Error::PermissionDenied);
        }
        Ok(u64::from(freq))
    }

    fn configure_clock(&mut self, clock_id: ClockId, config: &ClockConfig) -> Result<(), Error> {
        match clock_id {
            ClockId::ClkI3C0 | ClockId::ClkI3C1 | ClockId::ClkI3C2 | ClockId::ClkI3C3 => {
                if config.clk_source_sel > I3CClkSource::I3C480MHZ as u8 {
                    return Err(Error::InvalidClkSource);
                }
                self.scu.scu310().modify(|_, w| {
                    w.i3cclk_source_sel()
                        .bit(config.clk_source_sel != I3CClkSource::I3CHPLL as u8)
                });

                self.set_frequency(clock_id, config.frequency_hz)
            }

            ClockId::ClkHCLK => {
                if config.clk_source_sel > HCLKSource::HCLK as u8 {
                    return Err(Error::InvalidClkSource);
                }
                self.scu
                    .scu314()
                    .modify(|_, w| unsafe { w.hclkdivider_sel().bits(config.clk_source_sel) });

                self.set_frequency(clock_id, config.frequency_hz)
            }

            _ => Err(Error::PermissionDenied),
        }
    }

    fn get_clock_config(&self, clock_id: ClockId) -> Result<ClockConfig, Error> {
        let clk_source_sel: u8 = match clock_id {
            ClockId::ClkI3C0 | ClockId::ClkI3C1 | ClockId::ClkI3C2 | ClockId::ClkI3C3 => {
                if self.scu.scu310().read().i3cclk_source_sel().bit() {
                    I3CClkSource::I3C480MHZ as u8
                } else {
                    I3CClkSource::I3CHPLL as u8
                }
            }

            ClockId::ClkHCLK | ClockId::ClkPCLK => 0,

            _ => {
                return Err(Error::PermissionDenied);
            }
        };
        let frequency_hz = self.get_frequency(clock_id)?;
        let config = ClockConfig {
            frequency_hz,
            clk_source_sel,
        };
        Ok(config)
    }

    fn reset_assert(&mut self, reset_id: u8) -> Result<(), Error> {
        let mut bit_pos = reset_id;

        if bit_pos >= ASPEED_RESET_GRP_1_OFFSET + 32 {
            return Err(Error::InvalidResetId);
        }

        let reg_value: u32 = if bit_pos >= ASPEED_RESET_GRP_1_OFFSET {
            bit_pos -= ASPEED_RESET_GRP_1_OFFSET;
            self.scu.scu050().write(|w| unsafe { w.bits(1 << bit_pos) });
            self.scu.scu050().read().bits()
        } else {
            self.scu.scu040().write(|w| unsafe { w.bits(1 << bit_pos) });
            self.scu.scu040().read().bits()
        };

        if reg_value & (1 << bit_pos) != (1 << bit_pos) {
            return Err(Error::HardwareFailure);
        }
        Ok(())
    }

    fn reset_deassert(&mut self, reset_id: u8) -> Result<(), Error> {
        let mut bit_pos = reset_id;
        if bit_pos >= ASPEED_RESET_GRP_1_OFFSET + 32 {
            return Err(Error::InvalidResetId);
        }

        let reg_value: u32 = if bit_pos >= ASPEED_RESET_GRP_1_OFFSET {
            bit_pos -= ASPEED_RESET_GRP_1_OFFSET;
            self.scu.scu054().write(|w| unsafe { w.bits(1 << bit_pos) });
            self.scu.scu054().read().bits()
        } else {
            self.scu.scu044().write(|w| unsafe { w.bits(1 << bit_pos) });
            self.scu.scu044().read().bits()
        };

        if reg_value & (1 << bit_pos) != (1 << bit_pos) {
            return Err(Error::HardwareFailure);
        }
        Ok(())
    }

    fn reset_pulse(&mut self, reset_id: u8, duration: Duration) -> Result<(), Error> {
        let mut result: Result<(), Error>;
        let bit_pos: u8 = reset_id;
        if bit_pos >= ASPEED_RESET_GRP_1_OFFSET + 32 {
            return Err(Error::InvalidResetId);
        }
        result = self.reset_assert(reset_id);
        if result == Ok(()) {
            match u32::try_from(duration.as_nanos()) {
                Ok(ns) => {
                    self.delay.delay_ns(ns);
                    result = self.reset_deassert(reset_id);
                }
                Err(_) => {
                    result = Err(Error::InvalidResetId);
                }
            }
        }
        result
    }

    fn reset_is_asserted(&self, reset_id: u8) -> Result<bool, Error> {
        let mut bit_pos: u8 = reset_id;

        if bit_pos >= ASPEED_RESET_GRP_1_OFFSET + 32 {
            return Err(Error::InvalidResetId);
        }

        let reg_value: u32 = if bit_pos >= ASPEED_RESET_GRP_1_OFFSET + 32 {
            return Err(Error::InvalidResetId);
        } else if bit_pos >= ASPEED_RESET_GRP_1_OFFSET {
            bit_pos -= ASPEED_RESET_GRP_1_OFFSET;
            self.scu.scu050().read().bits()
        } else {
            self.scu.scu040().read().bits()
        };

        if reg_value & (1 << bit_pos) == (1 << bit_pos) {
            return Ok(true);
        }
        Ok(false)
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct ClockConfig {
    pub frequency_hz: u64,
    pub clk_source_sel: u8,
}

impl<D: DelayNs> ClockControl for SysCon<D> {
    type ClockId = ClockId;
    type ClockConfig = ClockConfig;

    fn enable(&mut self, clock_id: &Self::ClockId) -> Result<(), Self::Error> {
        self.enable_clock(*clock_id as u8)
    }

    fn disable(&mut self, clock_id: &Self::ClockId) -> Result<(), Self::Error> {
        self.disable_clock(*clock_id as u8)
    }

    fn set_frequency(
        &mut self,
        clock_id: &Self::ClockId,
        frequency_hz: u64,
    ) -> Result<(), Self::Error> {
        self.set_frequency(*clock_id, frequency_hz)
    }

    fn get_frequency(&self, clock_id: &Self::ClockId) -> Result<u64, Self::Error> {
        self.get_frequency(*clock_id)
    }

    fn configure(
        &mut self,
        clock_id: &Self::ClockId,
        config: Self::ClockConfig,
    ) -> Result<(), Self::Error> {
        self.configure_clock(*clock_id, &config)
    }

    fn get_config(&self, clock_id: &Self::ClockId) -> Result<Self::ClockConfig, Self::Error> {
        self.get_clock_config(*clock_id)
    }
}

impl<D: DelayNs> ResetControl for SysCon<D> {
    type ResetId = ResetId;

    fn reset_assert(&mut self, reset_id: &Self::ResetId) -> Result<(), Self::Error> {
        self.reset_assert(*reset_id as u8)
    }

    fn reset_deassert(&mut self, reset_id: &Self::ResetId) -> Result<(), Self::Error> {
        self.reset_deassert(*reset_id as u8)
    }
    /// * `reset_id` - A reference to the identifier of the reset line to pulse.
    /// * `duration_us` - The duration of the pulse in microseconds.
    fn reset_pulse(
        &mut self,
        reset_id: &Self::ResetId,
        duration: Duration,
    ) -> Result<(), Self::Error> {
        self.reset_pulse(*reset_id as u8, duration)
    }
    fn reset_is_asserted(&self, reset_id: &Self::ResetId) -> Result<bool, Self::Error> {
        self.reset_is_asserted(*reset_id as u8)
    }
}
