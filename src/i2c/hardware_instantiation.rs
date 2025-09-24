// Licensed under the Apache-2.0 license

//! # I2C Hardware Array Instantiation for AST1060
//!
//! This module provides a solution to a fundamental Rust type system challenge when working
//! with multiple instances of hardware peripherals in embedded systems.
//!
//! ## The Strong Typing Problem
//!
//! In Rust's type system, each I2C peripheral creates a distinct type when used with generics.
//! Consider these types:
//!
//! ```rust,ignore
//! // These are all DIFFERENT types due to the peripheral parameter:
//! I2cController<Ast1060I2c<ast1060_pac::I2c1, DummyI2CTarget, NoOpLogger>, NoOpLogger>
//! I2cController<Ast1060I2c<ast1060_pac::I2c2, DummyI2CTarget, NoOpLogger>, NoOpLogger>
//! I2cController<Ast1060I2c<ast1060_pac::I2c3, DummyI2CTarget, NoOpLogger>, NoOpLogger>
//! // ... up to I2c13
//! ```
//!
//! **Problem**: You cannot store different types in the same array in Rust:
//! ```rust,ignore
//! // This will NOT compile:
//! let controllers = [
//!     I2cController::<Ast1060I2c<I2c1, _, _>, _> { /* ... */ },
//!     I2cController::<Ast1060I2c<I2c2, _, _>, _> { /* ... */ },  // Error: different type!
//! ];
//! ```
//!
//! ## Solutions Considered
//!
//! ### 1. Trait Objects with Box (Rejected)
//! ```rust,ignore
//! // Requires heap allocation - violates no_std/no_alloc constraint
//! let controllers: Vec<Box<dyn I2c>> = vec![/* ... */];
//! ```
//!
//! ### 2. Tuple Approach (Cumbersome)
//! ```rust,ignore
//! // Works but becomes unwieldy with 13 controllers
//! type Controllers = (I2cController1, I2cController2, /* ... */, I2cController13);
//! ```
//!
//! ### 3. Separate Functions (No Grouping)
//! ```rust,ignore
//! // Cannot iterate or index - each controller needs separate handling
//! fn get_i2c1() -> I2cController1 { /* ... */ }
//! fn get_i2c2() -> I2cController2 { /* ... */ }
//! ```
//!
//! ### 4. Enum Wrapper (Chosen Solution)
//! ```rust,ignore
//! // Single type that can hold any I2C peripheral - allows arrays!
//! enum I2cControllerWrapper {
//!     I2c1(/* specific type */),
//!     I2c2(/* specific type */),
//!     // ...
//! }
//! let controllers: [I2cControllerWrapper; 13] = [/* ... */];
//! ```
//!
//! ## Design Benefits
//!
//! 1. **Uniform Access**: All controllers accessible via trait objects
//! 2. **Zero Allocation**: Stack-allocated enum variants in `no_std` environment
//! 3. **Type Safety**: Each peripheral maintains its specific type internally
//! 4. **Flexibility**: Supports both embedded-hal and `OpenProt` HAL traits
//! 5. **Iterability**: Can iterate over the array, index by bus number, etc.
//!
//! ## Usage Patterns
//!
//! ### Basic I2C Operations (embedded-hal)
//! ```rust,ignore
//! use aspeed_ddk::i2c::hardware_instantiation::instantiate_hardware;
//!
//! let mut controllers = instantiate_hardware();
//!
//! // Direct array indexing
//! controllers[0].as_i2c_mut().write(0x50, &[0x01, 0x02])?;  // I2C1
//! controllers[4].as_i2c_mut().read(0x51, &mut buffer)?;     // I2C5
//!
//! // Iteration over all controllers
//! for (index, controller) in controllers.iter_mut().enumerate() {
//!     println!("Configuring I2C bus {}", index + 1);
//!     controller.as_i2c_mut().write(0x20, &[0x00])?;
//! }
//! ```
//!
//! ### Master-Slave Operations (`OpenProt` HAL)
//! ```rust,ignore
//! #[cfg(feature = "i2c_target")]
//! {
//!     // Configure I2C1 as slave
//!     controllers[0].as_master_slave_mut().set_slave_address(0x42)?;
//!     controllers[0].as_master_slave_mut().enable_slave_mode()?;
//!
//!     // Use I2C2 as master to communicate with external device
//!     controllers[1].as_master_slave_mut().write(0x50, &[0x10, 0x20])?;
//!
//!     // Read slave data from I2C1
//!     let mut buffer = [0u8; 16];
//!     let bytes_read = controllers[0].as_master_slave_mut()
//!         .read_slave_buffer(&mut buffer)?;
//! }
//! ```
//!
//! ### Dynamic Bus Selection
//! ```rust,ignore
//! fn write_to_bus(controllers: &mut [I2cControllerWrapper; 13],
//!                 bus_number: usize,
//!                 address: u8,
//!                 data: &[u8]) -> Result<(), Error> {
//!     if bus_number == 0 || bus_number > 13 {
//!         return Err(Error::Invalid);
//!     }
//!     controllers[bus_number - 1].as_i2c_mut().write(address, data)
//! }
//! ```
//!
//! ## Performance Characteristics
//!
//! - **Memory**: Each enum variant is stack-allocated (no heap usage)
//! - **Runtime Cost**: Single match statement for trait object conversion
//! - **Binary Size**: Minimal - enum dispatch compiles to jump table
//! - **Type Erasure**: Only at the trait boundary, full type info maintained internally
//!
//! ## AST1060 I2C Architecture
//!
//! The AST1060 `SoC` provides 13 independent I2C controllers (I2C1-I2C13), each with:
//! - Independent register sets and interrupts
//! - Master and slave mode capabilities (when `i2c_target` feature enabled)
//! - Hardware buffer support for efficient transfers
//! - Individual timing and clock configuration
//!
//! ## Feature Gates
//!
//! - **Base functionality**: Always available - master mode via embedded-hal traits
//! - **`i2c_target` feature**: Enables slave mode and `OpenProt` `I2cMasterSlave` access
//!
//! ## Thread Safety
//!
//! The controllers use `NoOpLogger` and are designed for single-threaded embedded use.
//! For multi-threaded applications, wrap in appropriate synchronization primitives.

use crate::common::NoOpLogger;
use crate::i2c::ast1060_i2c::Ast1060I2c;
use crate::i2c::common::{I2cConfig, I2cConfigBuilder};
use crate::i2c::i2c_controller::I2cController;
use core::result::Result;
use core::result::Result::Ok;

/// Simple dummy I2C target for testing without external dependencies.
///
/// This provides a minimal implementation of the `I2CTarget` trait that can be used
/// in testing or when no actual I2C target functionality is needed.
pub struct DummyI2CTarget {
    address: u8,
}

impl Default for DummyI2CTarget {
    fn default() -> Self {
        Self::new()
    }
}

impl DummyI2CTarget {
    #[must_use]
    pub fn new() -> Self {
        Self { address: 0 }
    }
}

impl embedded_hal::i2c::ErrorType for DummyI2CTarget {
    type Error = crate::i2c::ast1060_i2c::Error;
}

impl proposed_traits::i2c_target::I2CCoreTarget for DummyI2CTarget {
    fn init(&mut self, address: u8) -> Result<(), Self::Error> {
        self.address = address;
        Ok(())
    }
    fn on_transaction_start(&mut self, _repeated: bool) {}
    fn on_stop(&mut self) {}
    fn on_address_match(&mut self, address: u8) -> bool {
        self.address == address
    }
}

impl proposed_traits::i2c_target::ReadTarget for DummyI2CTarget {
    fn on_read(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
        if buffer.is_empty() {
            Ok(0)
        } else {
            buffer[0] = 0x00;
            Ok(1)
        }
    }
}

impl proposed_traits::i2c_target::WriteTarget for DummyI2CTarget {
    fn on_write(&mut self, _data: &[u8]) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl proposed_traits::i2c_target::WriteReadTarget for DummyI2CTarget {}

impl proposed_traits::i2c_target::RegisterAccess for DummyI2CTarget {
    fn write_register(&mut self, _register: u8, _data: u8) -> Result<(), Self::Error> {
        Ok(())
    }
    fn read_register(&mut self, _register: u8, _data: &mut [u8]) -> Result<usize, Self::Error> {
        Ok(0)
    }
}

/// Type alias for I2C controller without UART logging dependencies.
///
/// This creates a clean controller type that uses:
/// - `NoOpLogger` instead of `UartLogger` to avoid UART coupling
/// - `DummyI2CTarget` for testing and development
/// - Generic `I2C` parameter for different peripheral instances
/// - Lifetime parameter for DMA buffers
pub type I2cControllerNoLog<'a, I2C> =
    I2cController<Ast1060I2c<'a, I2C, DummyI2CTarget, NoOpLogger>, NoOpLogger>;

/// Enum wrapper enabling storage of heterogeneous I2C controller types in a single array.
///
/// Each variant represents a different I2C peripheral on the AST1060 `SoC`. This design
/// solves the fundamental problem that `I2cController<Ast1060I2c<I2c1, _, _>, _>` and
/// `I2cController<Ast1060I2c<I2c2, _, _>, _>` are different types in Rust's type system.
///
/// The enum allows:
/// - Uniform array storage: `[I2cControllerWrapper; 13]`
/// - Trait object access for both embedded-hal and `OpenProt` HAL
/// - Zero-allocation operation in `no_std` environments
/// - Type-safe access to underlying hardware-specific controllers
///
/// # Examples
///
/// ```rust,ignore
/// let controllers = instantiate_hardware();
///
/// // Access specific controller by index
/// let i2c1 = &mut controllers[0];  // I2C1 controller
/// let i2c5 = &mut controllers[4];  // I2C5 controller
///
/// // Uniform access via trait objects
/// i2c1.as_i2c_mut().write(0x50, &[0x01])?;
/// i2c5.as_i2c_mut().read(0x51, &mut buffer)?;
/// ```
pub enum I2cControllerWrapper<'a> {
    /// I2C1 peripheral controller
    I2c1(I2cControllerNoLog<'a, ast1060_pac::I2c1>),
    /// I2C2 peripheral controller
    I2c2(I2cControllerNoLog<'a, ast1060_pac::I2c2>),
    /// I2C3 peripheral controller
    I2c3(I2cControllerNoLog<'a, ast1060_pac::I2c3>),
    /// I2C4 peripheral controller
    I2c4(I2cControllerNoLog<'a, ast1060_pac::I2c4>),
    /// I2C5 peripheral controller
    I2c5(I2cControllerNoLog<'a, ast1060_pac::I2c5>),
    /// I2C6 peripheral controller
    I2c6(I2cControllerNoLog<'a, ast1060_pac::I2c6>),
    /// I2C7 peripheral controller
    I2c7(I2cControllerNoLog<'a, ast1060_pac::I2c7>),
    /// I2C8 peripheral controller
    I2c8(I2cControllerNoLog<'a, ast1060_pac::I2c8>),
    /// I2C9 peripheral controller
    I2c9(I2cControllerNoLog<'a, ast1060_pac::I2c9>),
    /// I2C10 peripheral controller
    I2c10(I2cControllerNoLog<'a, ast1060_pac::I2c10>),
    /// I2C11 peripheral controller
    I2c11(I2cControllerNoLog<'a, ast1060_pac::I2c11>),
    /// I2C12 peripheral controller
    I2c12(I2cControllerNoLog<'a, ast1060_pac::I2c12>),
    /// I2C13 peripheral controller
    I2c13(I2cControllerNoLog<'a, ast1060_pac::I2c13>),
}

impl<'a> I2cControllerWrapper<'a> {
    /// Get mutable access to the I2C controller via embedded-hal traits.
    ///
    /// This method provides uniform access to all I2C controllers through the standard
    /// `embedded_hal::i2c::I2c` trait, regardless of which specific peripheral the
    /// controller wraps. This enables generic I2C operations across all buses.
    ///
    /// # Returns
    /// A mutable trait object reference supporting standard I2C operations:
    /// - `read(address, buffer)` - Read from slave device
    /// - `write(address, data)` - Write to slave device
    /// - `write_read(address, write_data, read_buffer)` - Combined write-then-read
    /// - `transaction(address, operations)` - Execute multiple operations atomically
    ///
    /// # Examples
    /// ```rust,ignore
    /// let mut controllers = instantiate_hardware();
    ///
    /// // Read temperature from sensor on I2C1
    /// let mut temp_data = [0u8; 2];
    /// controllers[0].as_i2c_mut().read(0x48, &mut temp_data)?;
    ///
    /// // Write configuration to device on I2C5
    /// controllers[4].as_i2c_mut().write(0x50, &[0x10, 0x01])?;
    ///
    /// // Combined write-read operation
    /// let mut read_buf = [0u8; 4];
    /// controllers[2].as_i2c_mut().write_read(0x51, &[0x00], &mut read_buf)?;
    /// ```
    pub fn as_i2c_mut(
        &mut self,
    ) -> &mut dyn embedded_hal::i2c::I2c<Error = crate::i2c::ast1060_i2c::Error> {
        match self {
            I2cControllerWrapper::I2c1(controller) => controller,
            I2cControllerWrapper::I2c2(controller) => controller,
            I2cControllerWrapper::I2c3(controller) => controller,
            I2cControllerWrapper::I2c4(controller) => controller,
            I2cControllerWrapper::I2c5(controller) => controller,
            I2cControllerWrapper::I2c6(controller) => controller,
            I2cControllerWrapper::I2c7(controller) => controller,
            I2cControllerWrapper::I2c8(controller) => controller,
            I2cControllerWrapper::I2c9(controller) => controller,
            I2cControllerWrapper::I2c10(controller) => controller,
            I2cControllerWrapper::I2c11(controller) => controller,
            I2cControllerWrapper::I2c12(controller) => controller,
            I2cControllerWrapper::I2c13(controller) => controller,
        }
    }

    /// Get immutable access to the I2C controller via embedded-hal traits.
    ///
    /// This method provides read-only access to I2C controller state and capabilities
    /// through the `embedded_hal::i2c::I2c` trait. Useful for inspecting controller
    /// configuration or implementing read-only operations.
    ///
    /// # Returns
    /// An immutable trait object reference for read-only I2C operations.
    ///
    /// # Examples
    /// ```rust,ignore
    /// let controllers = instantiate_hardware();
    ///
    /// // Check if controller supports certain error types
    /// match controllers[0].as_i2c().read(0x48, &mut buffer) {
    ///     Ok(()) => println!("Read successful"),
    ///     Err(e) => println!("I2C error: {:?}", e),
    /// }
    /// ```
    #[must_use]
    pub fn as_i2c(&self) -> &dyn embedded_hal::i2c::I2c<Error = crate::i2c::ast1060_i2c::Error> {
        match self {
            I2cControllerWrapper::I2c1(controller) => controller,
            I2cControllerWrapper::I2c2(controller) => controller,
            I2cControllerWrapper::I2c3(controller) => controller,
            I2cControllerWrapper::I2c4(controller) => controller,
            I2cControllerWrapper::I2c5(controller) => controller,
            I2cControllerWrapper::I2c6(controller) => controller,
            I2cControllerWrapper::I2c7(controller) => controller,
            I2cControllerWrapper::I2c8(controller) => controller,
            I2cControllerWrapper::I2c9(controller) => controller,
            I2cControllerWrapper::I2c10(controller) => controller,
            I2cControllerWrapper::I2c11(controller) => controller,
            I2cControllerWrapper::I2c12(controller) => controller,
            I2cControllerWrapper::I2c13(controller) => controller,
        }
    }

    /// Get mutable access to the underlying I2C hardware for master-slave operations.
    ///
    /// This method provides access to the actual `Ast1060I2c` hardware implementation,
    /// which implements the `OpenProt` HAL master-slave traits when the `i2c_target`
    /// feature is enabled. This allows direct access to slave functionality without
    /// trait object limitations.
    ///
    /// **Requires**: `i2c_target` feature must be enabled at compile time.
    ///
    /// # Returns
    /// A mutable reference to the concrete hardware type, allowing access to all
    /// master and slave functionality.
    ///
    /// # Examples
    /// ```rust,ignore
    /// #[cfg(feature = "i2c_target")]
    /// {
    ///     use openprot_hal_blocking::i2c_hardware::slave::I2cSlaveCore;
    ///     use openprot_hal_blocking::i2c_hardware::I2cMaster;
    ///
    ///     let mut controllers = instantiate_hardware();
    ///
    ///     // Access hardware directly for slave configuration
    ///     match &mut controllers[0] {
    ///         I2cControllerWrapper::I2c1(controller) => {
    ///             controller.hardware.set_slave_address(0x42)?;
    ///             controller.hardware.enable_slave_mode()?;
    ///             controller.hardware.write(0x50, &[0x10, 0x20])?;
    ///         }
    ///         _ => unreachable!(),
    ///     }
    /// }
    /// ```
    #[cfg(feature = "i2c_target")]
    pub fn get_hardware_mut(&mut self) -> &mut (dyn core::any::Any + 'a) {
        match self {
            I2cControllerWrapper::I2c1(controller) => &mut controller.hardware,
            I2cControllerWrapper::I2c2(controller) => &mut controller.hardware,
            I2cControllerWrapper::I2c3(controller) => &mut controller.hardware,
            I2cControllerWrapper::I2c4(controller) => &mut controller.hardware,
            I2cControllerWrapper::I2c5(controller) => &mut controller.hardware,
            I2cControllerWrapper::I2c6(controller) => &mut controller.hardware,
            I2cControllerWrapper::I2c7(controller) => &mut controller.hardware,
            I2cControllerWrapper::I2c8(controller) => &mut controller.hardware,
            I2cControllerWrapper::I2c9(controller) => &mut controller.hardware,
            I2cControllerWrapper::I2c10(controller) => &mut controller.hardware,
            I2cControllerWrapper::I2c11(controller) => &mut controller.hardware,
            I2cControllerWrapper::I2c12(controller) => &mut controller.hardware,
            I2cControllerWrapper::I2c13(controller) => &mut controller.hardware,
        }
    }

    /// Get the bus number (1-13) for this controller.
    ///
    /// This provides a way to determine which physical I2C bus a controller represents,
    /// useful for logging, configuration lookup, or routing decisions.
    ///
    /// # Returns
    /// Bus number from 1 to 13 corresponding to I2C1 through I2C13.
    ///
    /// # Examples
    /// ```rust,ignore
    /// let controllers = instantiate_hardware();
    ///
    /// for (index, controller) in controllers.iter().enumerate() {
    ///     let bus_num = controller.bus_number();
    ///     assert_eq!(bus_num, index + 1);
    ///     println!("Controller at index {} is I2C bus {}", index, bus_num);
    /// }
    /// ```
    #[must_use]
    pub fn bus_number(&self) -> u8 {
        match self {
            I2cControllerWrapper::I2c1(_) => 1,
            I2cControllerWrapper::I2c2(_) => 2,
            I2cControllerWrapper::I2c3(_) => 3,
            I2cControllerWrapper::I2c4(_) => 4,
            I2cControllerWrapper::I2c5(_) => 5,
            I2cControllerWrapper::I2c6(_) => 6,
            I2cControllerWrapper::I2c7(_) => 7,
            I2cControllerWrapper::I2c8(_) => 8,
            I2cControllerWrapper::I2c9(_) => 9,
            I2cControllerWrapper::I2c10(_) => 10,
            I2cControllerWrapper::I2c11(_) => 11,
            I2cControllerWrapper::I2c12(_) => 12,
            I2cControllerWrapper::I2c13(_) => 13,
        }
    }
}

/// Instantiate all available I2C hardware controllers on the AST1060 `SoC`.
///
/// This function creates and returns an array containing controllers for all 13 I2C
/// peripherals available on the AST1060. Each controller is configured with sensible
/// defaults and no UART logging dependencies, making them suitable for both testing
/// and production use in `no_std` environments.
///
/// # Configuration
/// All controllers are initialized with:
/// - **Transfer mode**: Byte mode (most compatible)
/// - **Speed**: 100 kHz (I2C Standard mode)
/// - **Multi-master**: Disabled
/// - **`SMBus` features**: Timeout and alert disabled
/// - **Logging**: No-operation logger (no UART dependency)
/// - **Target**: Dummy I2C target for testing
///
/// # Returns
/// Array of 13 `I2cControllerWrapper` instances, where:
/// - `controllers[0]` = I2C1 peripheral
/// - `controllers[1]` = I2C2 peripheral
/// - ...
/// - `controllers[12]` = I2C13 peripheral
///
/// # Performance
/// - **Memory**: All controllers are stack-allocated
/// - **Initialization**: Lightweight - only creates controller structs
/// - **Runtime**: Zero-cost abstractions via enum dispatch
///
/// # Thread Safety
/// The returned controllers are **not** thread-safe. For multi-threaded use,
/// wrap them in appropriate synchronization primitives (e.g., `Mutex`).
///
/// # Examples
///
/// ## Basic Usage
/// ```rust,ignore
/// use aspeed_ddk::i2c::hardware_instantiation::instantiate_hardware;
///
/// // Get all I2C controllers
/// let mut controllers = instantiate_hardware();
///
/// // Use I2C1 to read from a sensor
/// let mut sensor_data = [0u8; 2];
/// controllers[0].as_i2c_mut().read(0x48, &mut sensor_data)?;
/// println!("Sensor reading: {:?}", sensor_data);
///
/// // Use I2C5 to write configuration
/// controllers[4].as_i2c_mut().write(0x50, &[0x10, 0x01])?;
/// ```
///
/// ## Iteration and Batch Operations
/// ```rust,ignore
/// let mut controllers = instantiate_hardware();
///
/// // Initialize all controllers with custom configuration
/// for (index, controller) in controllers.iter_mut().enumerate() {
///     println!("Initializing I2C bus {}", index + 1);
///
///     // Perform bus recovery if needed
///     if let Err(e) = controller.as_i2c_mut().write(0x00, &[]) {
///         eprintln!("Bus {} needs recovery: {:?}", index + 1, e);
///     }
/// }
/// ```
///
/// ## Master-Slave Configuration
/// ```rust,ignore
/// #[cfg(feature = "i2c_target")]
/// {
///     let mut controllers = instantiate_hardware();
///
///     // Set up I2C1 as a slave device
///     let slave_controller = &mut controllers[0];
///     slave_controller.as_master_slave_mut().set_slave_address(0x42)?;
///     slave_controller.as_master_slave_mut().enable_slave_mode()?;
///
///     // Prepare response data for when master reads from us
///     let response_data = [0x01, 0x02, 0x03, 0x04];
///     slave_controller.as_master_slave_mut().write_slave_response(&response_data)?;
///
///     // Use I2C2 as master to communicate with external devices
///     let master_controller = &mut controllers[1];
///     master_controller.as_master_slave_mut().write(0x50, &[0xAA, 0xBB])?;
///
///     // Poll for slave events on I2C1
///     if let Some(event) = slave_controller.as_master_slave_mut().poll_slave_events()? {
///         println!("Slave event received: {:?}", event);
///         slave_controller.as_master_slave_mut().handle_slave_event(event)?;
///     }
/// }
/// ```
///
/// ## Dynamic Bus Selection
/// ```rust,ignore
/// fn communicate_with_device(controllers: &mut [I2cControllerWrapper; 13],
///                            bus_number: u8,
///                            device_addr: u8,
///                            command: u8) -> Result<u8, Box<dyn std::error::Error>> {
///     if bus_number < 1 || bus_number > 13 {
///         return Err("Invalid bus number".into());
///     }
///
///     let controller = &mut controllers[(bus_number - 1) as usize];
///
///     // Write command and read response
///     let mut response = [0u8; 1];
///     controller.as_i2c_mut().write_read(device_addr, &[command], &mut response)?;
///
///     Ok(response[0])
/// }
///
/// let mut controllers = instantiate_hardware();
/// let result = communicate_with_device(&mut controllers, 3, 0x48, 0x01)?;
/// println!("Device on I2C3 returned: 0x{:02X}", result);
/// ```
#[must_use]
pub fn instantiate_hardware<'a>() -> [I2cControllerWrapper<'a>; 13] {
    [
        I2cControllerWrapper::I2c1(create_i2c1_controller(I2cConfigBuilder::new().build())),
        I2cControllerWrapper::I2c2(create_i2c2_controller(I2cConfigBuilder::new().build())),
        I2cControllerWrapper::I2c3(create_i2c3_controller(I2cConfigBuilder::new().build())),
        I2cControllerWrapper::I2c4(create_i2c4_controller(I2cConfigBuilder::new().build())),
        I2cControllerWrapper::I2c5(create_i2c5_controller(I2cConfigBuilder::new().build())),
        I2cControllerWrapper::I2c6(create_i2c6_controller(I2cConfigBuilder::new().build())),
        I2cControllerWrapper::I2c7(create_i2c7_controller(I2cConfigBuilder::new().build())),
        I2cControllerWrapper::I2c8(create_i2c8_controller(I2cConfigBuilder::new().build())),
        I2cControllerWrapper::I2c9(create_i2c9_controller(I2cConfigBuilder::new().build())),
        I2cControllerWrapper::I2c10(create_i2c10_controller(I2cConfigBuilder::new().build())),
        I2cControllerWrapper::I2c11(create_i2c11_controller(I2cConfigBuilder::new().build())),
        I2cControllerWrapper::I2c12(create_i2c12_controller(I2cConfigBuilder::new().build())),
        I2cControllerWrapper::I2c13(create_i2c13_controller(I2cConfigBuilder::new().build())),
    ]
}

// Helper functions to create individual controller instances
// These are separate functions to ensure each controller gets the correct peripheral type

fn create_i2c1_controller<'a>(config: I2cConfig) -> I2cControllerNoLog<'a, ast1060_pac::I2c1> {
    I2cController {
        hardware: Ast1060I2c::new(NoOpLogger {}),
        config,
        logger: NoOpLogger {},
    }
}

fn create_i2c2_controller<'a>(config: I2cConfig) -> I2cControllerNoLog<'a, ast1060_pac::I2c2> {
    I2cController {
        hardware: Ast1060I2c::new(NoOpLogger {}),
        config,
        logger: NoOpLogger {},
    }
}

fn create_i2c3_controller<'a>(config: I2cConfig) -> I2cControllerNoLog<'a, ast1060_pac::I2c3> {
    I2cController {
        hardware: Ast1060I2c::new(NoOpLogger {}),
        config,
        logger: NoOpLogger {},
    }
}

fn create_i2c4_controller<'a>(config: I2cConfig) -> I2cControllerNoLog<'a, ast1060_pac::I2c4> {
    I2cController {
        hardware: Ast1060I2c::new(NoOpLogger {}),
        config,
        logger: NoOpLogger {},
    }
}

fn create_i2c5_controller<'a>(config: I2cConfig) -> I2cControllerNoLog<'a, ast1060_pac::I2c5> {
    I2cController {
        hardware: Ast1060I2c::new(NoOpLogger {}),
        config,
        logger: NoOpLogger {},
    }
}

fn create_i2c6_controller<'a>(config: I2cConfig) -> I2cControllerNoLog<'a, ast1060_pac::I2c6> {
    I2cController {
        hardware: Ast1060I2c::new(NoOpLogger {}),
        config,
        logger: NoOpLogger {},
    }
}

fn create_i2c7_controller<'a>(config: I2cConfig) -> I2cControllerNoLog<'a, ast1060_pac::I2c7> {
    I2cController {
        hardware: Ast1060I2c::new(NoOpLogger {}),
        config,
        logger: NoOpLogger {},
    }
}

fn create_i2c8_controller<'a>(config: I2cConfig) -> I2cControllerNoLog<'a, ast1060_pac::I2c8> {
    I2cController {
        hardware: Ast1060I2c::new(NoOpLogger {}),
        config,
        logger: NoOpLogger {},
    }
}

fn create_i2c9_controller<'a>(config: I2cConfig) -> I2cControllerNoLog<'a, ast1060_pac::I2c9> {
    I2cController {
        hardware: Ast1060I2c::new(NoOpLogger {}),
        config,
        logger: NoOpLogger {},
    }
}

fn create_i2c10_controller<'a>(config: I2cConfig) -> I2cControllerNoLog<'a, ast1060_pac::I2c10> {
    I2cController {
        hardware: Ast1060I2c::new(NoOpLogger {}),
        config,
        logger: NoOpLogger {},
    }
}

fn create_i2c11_controller<'a>(config: I2cConfig) -> I2cControllerNoLog<'a, ast1060_pac::I2c11> {
    I2cController {
        hardware: Ast1060I2c::new(NoOpLogger {}),
        config,
        logger: NoOpLogger {},
    }
}

fn create_i2c12_controller<'a>(config: I2cConfig) -> I2cControllerNoLog<'a, ast1060_pac::I2c12> {
    I2cController {
        hardware: Ast1060I2c::new(NoOpLogger {}),
        config,
        logger: NoOpLogger {},
    }
}

fn create_i2c13_controller<'a>(config: I2cConfig) -> I2cControllerNoLog<'a, ast1060_pac::I2c13> {
    I2cController {
        hardware: Ast1060I2c::new(NoOpLogger {}),
        config,
        logger: NoOpLogger {},
    }
}
