# Copilot Instructions for aspeed-ddk

## Project Overview
aspeed-ddk is a Rust-based driver development kit for ASPEED AST1060 SoCs, targeting ARM Cortex-M4 bare-metal environments. This is a **no_std**, **no_alloc** embedded systems crate with strict panic-free requirements.

**Target**: `thumbv7em-none-eabihf` | **Toolchain**: nightly-2024-09-17 (see `rust-toolchain.toml`)

## Architecture Overview

### Hardware Abstraction Pattern
Controllers use type-parameterized structs with `Instance` traits for type-safe hardware access:
```rust
// Timer, Watchdog, etc. follow this pattern
pub struct TimerController<T: TimerInstance> { phantom: PhantomData<T> }
let timer = TimerController::<ast1060_pac::Timer>::new();
```

### Memory Layout (memory.x)
- **FLASH**: 0x80000000, 1024K
- **RAM**: 0x00000000, 640K
- **RAM_NC**: 0x000A0000, 128K (non-cacheable, for DMA buffers via `#[link_section = ".ram_nc"]`)

Use `DmaBuffer<N>` from `common.rs` for DMA operations - it's 32-byte aligned and placed in RAM_NC.

### HAL Trait Ecosystem
- **embedded-hal 1.0**: Standard traits (SpiBus, DelayNs, etc.)
- **proposed-traits**: Custom traits for ClockControl, ResetControl, Digest, MAC (from OpenPRoT project)
- **openprot-hal-blocking**: Move-based digest API for exclusive hardware access

### Major Components
- **syscon.rs**: Clock/reset control via `SysCon` - use `ClockId` and `ResetId` enums
- **hace_controller.rs**: Cryptographic engine (hash, HMAC, RSA, ECDSA) with shared context in `.ram_nc`
- **pinctrl.rs**: Pin muxing via `modify_reg!` macro - see PINCTRL_* constants for pin groups
- **spi/**: SPI controllers with `SpiBusWithCs` trait extension
- **i2c/**: I2C controllers including AST1060-specific I3C support

## Pull Request Review Checklist

- [ ] Code is completely panic-free (no unwrap/expect/panic/indexing)
- [ ] All fallible operations return Result or Option
- [ ] Integer operations use checked/saturating/wrapping methods where needed
- [ ] Array/slice access uses get() or pattern matching, not direct indexing
- [ ] Error cases are well documented and handled appropriately
- [ ] Tests verify error handling paths, not just happy paths
- [ ] No heap allocation in production code (excluding tests)
- [ ] Unsafe code has safety comments and is thoroughly tested

## Quick Reference: Forbidden Patterns

| Forbidden Pattern | Required Alternative |
|-------------------|----------------------|
| `value.unwrap()` | `match value { Some(v) => v, None => return Err(...) }` |
| `result.expect("msg")` | `match result { Ok(v) => v, Err(e) => return Err(e.into()) }` |
| `collection[index]` | `collection.get(index).ok_or(Error::OutOfBounds)?` |
| `heapless::Vec` (production) | Fixed-size `[T; N]` arrays or caller-provided slices |

## Code Style

### no_std and Memory Allocation Guidelines

- This project is strictly **no_std** and **no_alloc** in production code
- All production paths must be allocation-free and compatible with bare-metal targets

#### Production Code Requirements

- **NEVER** use crates or features that require heap allocation in production code
- **DO NOT** use the `heapless` crate in production paths despite its name suggesting compatibility
- **DO NOT** use any crate that depends on the `alloc` crate without feature gating
- **ALWAYS** use fixed-size arrays, slices, or static memory allocation
- **ALWAYS** design APIs to accept and return memory provided by the caller

#### Memory Management in Production Code

- Buffers must be pre-allocated by the caller and passed as slices
- Collection types must have fixed, compile-time sizes
- All data structures must have predictable, static memory footprints
- No dynamic memory growth patterns are allowed

#### Test Code Exceptions

- Test code (annotated with `#[cfg(test)]`) may use allocation if needed
- The `heapless` crate and other no_std compatible collections are permitted in tests
- Standard library features may be used in tests when the `std` feature is enabled
- Test helpers can use more ergonomic APIs that wouldn't be appropriate for production

#### Example: Production vs. Test Code

```rust
// Production code - strict no_alloc approach
pub fn process_data(data: &[u8], output: &mut [u8]) -> Result<usize, Error> {
    if output.len() < data.len() * 2 {
        return Err(Error::BufferTooSmall);
    }
    
    // Process data into output buffer
    // ...
    
    Ok(processed_bytes)
}

// Test code - can use more ergonomic approaches
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_process_data() {
        // It's fine to use Vec in tests
        let input = vec![1, 2, 3, 4];
        let mut output = vec![0; 16];
        
        let result = process_data(&input, &mut output);
        assert!(result.is_ok());
        // ...
    }
    
    #[test]
    fn test_with_heapless() {
        // Heapless is also fine in tests
        use heapless::Vec;
        let mut input: Vec<u8, 8> = Vec::new();
        input.extend_from_slice(&[1, 2, 3]).unwrap();
        
        let mut output = [0u8; 16];
        let result = process_data(&input, &mut output);
        assert!(result.is_ok());
        // ...
    }
}
```

### Unsafe Code

- Minimize unsafe code; isolate in well-documented functions
- Document all safety preconditions in unsafe functions
- Add safety comments explaining why unsafe is necessary
- Unit test unsafe code thoroughly

## Common Patterns

### Static vs. Dynamic Dispatch

This project strongly prefers static dispatch over dynamic dispatch to optimize for binary size, performance, and no_std compatibility.

#### Static Dispatch Requirements

- Use generic parameters instead of trait objects (`dyn Trait`) whenever possible
- Leverage impl Trait in return positions rather than Box<dyn Trait>
- Monomorphize code at compile time rather than using virtual dispatch at runtime
- Avoid heap allocations associated with typical dyn Trait usage

### Pin Muxing Pattern
```rust
use pinctrl::modify_reg;
modify_reg!(scu.scu690(), PIN_SPIM0_CLK_OUT_BIT, clear);  // Clear bit
modify_reg!(gpio.gpio004(), PIN_BIT, set);                 // Set bit
```

### Shared Hardware Resources
When multiple APIs access the same hardware (e.g., HACE for crypto), use:
- **Scoped API** (`hash.rs`): Borrow controller for operation lifetime
- **Owned API** (`hash_owned.rs`): Move controller for exclusive access, no lifetime constraints

## Workflows

### Development Commands
```bash
# Build for target
cargo build --release --target thumbv7em-none-eabihf

# Run all quality checks before commit
cargo xtask precommit

# Build with hardware test features
cargo build --features "test-rsa,test-hash,test-ecdsa,test-hmac"

# Generate UART boot image
cargo objcopy -- -O binary ast10x0.bin
scripts/gen_uart_booting_image.py ast10x0.bin uart_ast10x0.bin

# Run on QEMU
qemu-system-arm -M ast1030-evb -nographic -kernel target/.../aspeed-ddk
```

### XTask Automation (see xtask/README.md)
```bash
cargo xtask build [--release] [--features "..."]
cargo xtask clippy
cargo xtask format [--fix]
cargo xtask docs [--open]
cargo xtask header-check / header-fix
cargo xtask precommit  # Run before every commit
```

## Feature Flags
- `test-rsa`, `test-ecdsa`, `test-hash`, `test-hmac`: Enable hardware test suites in `main.rs`
- `spi_dma`, `spi_dma_write`: Enable SPI DMA functionality
- `spi_monitor`: Enable SPI monitoring features
- `i2c_target`: Enable I2C target mode

## Key Files for Reference
- **memory.x / link.x**: Linker scripts defining memory layout
- **build.rs**: Build-time linker configuration
- **main.rs**: Example usage showing controller initialization patterns
- **common.rs**: DmaBuffer, Logger traits, and utility types