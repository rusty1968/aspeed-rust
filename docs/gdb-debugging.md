# GDB Debugging Guide for ASPEED DDK

This guide covers debugging the ASPEED DDK using GDB with JLink hardware debugger.

## Build Profiles for Debugging

The project includes multiple build profiles optimized for different use cases:

### `gdb-test` Profile (Recommended for GDB)

**Purpose**: Optimized specifically for GDB debugging with minimal performance impact.

**Characteristics**:
- **Optimization Level**: `1` (minimal optimizations)
- **Debug Info**: Full debug symbols (level 2)
- **Advantages**:
  - Preserves most variable names and line numbers
  - Allows stepping through code reliably
  - Still functional enough to test hardware interactions
  - Better than `dev` for embedded systems (some optimizations prevent hangs)
  - Full symbol information for backtraces

**Build Command**:
```bash
# Build with gdb-test profile
cargo build --profile gdb-test --target thumbv7em-none-eabihf

# With test features
cargo build --profile gdb-test --target thumbv7em-none-eabihf --features "test-hmac,test-hash"
```

**Binary Location**: `target/thumbv7em-none-eabihf/gdb-test/aspeed-ddk`

### `dev` Profile (Default Debug)

**Purpose**: Standard Rust debug build.

**Characteristics**:
- **Optimization Level**: `0` (no optimizations)
- **Debug Info**: Full debug symbols
- **Advantages**:
  - Most predictable for debugging
  - All variables preserved
  - Exact source-to-assembly mapping
- **Disadvantages**:
  - May be too slow for hardware interactions
  - Larger binary size
  - Can expose timing-sensitive bugs

**Build Command**:
```bash
cargo build --target thumbv7em-none-eabihf
# or explicitly
cargo build --profile dev --target thumbv7em-none-eabihf
```

**Binary Location**: `target/thumbv7em-none-eabihf/debug/aspeed-ddk`

### `release` Profile (Not Recommended for GDB)

**Purpose**: Production builds optimized for size.

**Characteristics**:
- **Optimization Level**: `z` (optimize for size)
- **Debug Info**: None (stripped)
- **Link-Time Optimization**: Enabled
- **Why Not for GDB**:
  - Variables optimized away
  - Function inlining makes stepping unpredictable
  - No symbol information
  - Code reordering breaks source mapping

**Build Command**:
```bash
cargo build --release --target thumbv7em-none-eabihf
```

## Using the GDB Test Monitor

### Quick Start

The easiest way to run GDB tests is with automatic building:

```bash
# Build and test HMAC suite
./scripts/run_gdb_tests.sh --build --suite hmac

# Build and test all suites
./scripts/run_gdb_tests.sh --build --suite all
```

### Manual Workflow

If you prefer to build separately:

```bash
# 1. Build with gdb-test profile
cargo build --profile gdb-test --target thumbv7em-none-eabihf --features test-hmac

# 2. Run GDB monitor
./scripts/run_gdb_tests.sh --suite hmac
```

### Profile Selection

You can explicitly choose a profile:

```bash
# Use gdb-test profile (default)
./scripts/run_gdb_tests.sh --build --profile gdb-test --suite hash

# Use dev profile (no optimizations)
./scripts/run_gdb_tests.sh --build --profile dev --suite rsa

# Use release profile (not recommended)
./scripts/run_gdb_tests.sh --build --profile release --suite ecdsa
```

## Common GDB Debugging Scenarios

### Interactive Debugging Session

For manual debugging instead of automated testing:

```bash
# 1. Start JLink GDB server
JLinkGDBServer -device cortex-m4 -if swd -port 2331

# 2. In another terminal, start GDB
gdb-multiarch target/thumbv7em-none-eabihf/gdb-test/aspeed-ddk

# 3. In GDB, connect and load
(gdb) target remote localhost:2331
(gdb) monitor semihosting enable
(gdb) load
(gdb) break main
(gdb) continue
```

### Setting Breakpoints

```gdb
# Break on function
(gdb) break aspeed_ddk::hmac::AspeedHmac::compute

# Break on file:line
(gdb) break src/hmac.rs:145

# Break on panic
(gdb) break panic_halt::panic
(gdb) break rust_begin_unwind

# List breakpoints
(gdb) info breakpoints

# Delete breakpoint
(gdb) delete 1
```

### Examining State

```gdb
# View backtrace
(gdb) backtrace
(gdb) bt

# View local variables
(gdb) info locals

# Print variable
(gdb) print my_variable
(gdb) p/x my_variable  # in hex

# Examine memory
(gdb) x/16xb 0x00000000  # 16 bytes as hex
(gdb) x/4xw 0x00000000   # 4 words as hex

# View registers
(gdb) info registers
(gdb) info all-registers
```

### Stepping Through Code

```gdb
# Step into functions
(gdb) step
(gdb) s

# Step over functions
(gdb) next
(gdb) n

# Continue to next breakpoint
(gdb) continue
(gdb) c

# Finish current function
(gdb) finish
```

## Troubleshooting

### Variables Optimized Away

**Symptom**: `<optimized out>` when printing variables

**Solution**: Use `gdb-test` or `dev` profile instead of `release`:
```bash
cargo build --profile gdb-test --target thumbv7em-none-eabihf
```

### Cannot Step Through Code

**Symptom**: Stepping jumps around unexpectedly

**Cause**: Optimizations reorder code and inline functions

**Solution**: Build with lower optimization:
```bash
# Try gdb-test first
cargo build --profile gdb-test --target thumbv7em-none-eabihf

# If still problematic, try dev
cargo build --profile dev --target thumbv7em-none-eabihf
```

### Hardware Timing Issues

**Symptom**: Code works in release but not debug, or vice versa

**Cause**: Debug builds are slower and can expose timing bugs

**Solution**: Use `gdb-test` profile as middle ground:
```bash
cargo build --profile gdb-test --target thumbv7em-none-eabihf
```

The minimal optimizations in `gdb-test` help with timing while preserving debuggability.

### Missing Symbol Information

**Symptom**: No function names in backtrace, generic addresses

**Cause**: Binary was stripped or built without debug info

**Solution**: Ensure using debug profile and `strip = false`:
```bash
cargo build --profile gdb-test --target thumbv7em-none-eabihf
```

### Binary Too Large

**Symptom**: Binary doesn't fit in flash (> 1024K)

**Trade-off**: Debug builds are larger

**Solutions**:
1. Use `gdb-test` profile (smaller than `dev`)
2. Disable some test features
3. For production, use `release` profile

```bash
# Check binary size
ls -lh target/thumbv7em-none-eabihf/gdb-test/aspeed-ddk
```

## Profile Comparison

| Feature | dev | gdb-test | release |
|---------|-----|----------|---------|
| Optimization | None (0) | Minimal (1) | Max size (z) |
| Debug Symbols | Full | Full | None |
| Binary Size | ~800K | ~600K | ~200K |
| Variable Preservation | Excellent | Good | Poor |
| Step Accuracy | Excellent | Good | Poor |
| Performance | Slow | Moderate | Fast |
| Hardware Testing | May timeout | Good | Good |
| Recommended For | Deep debugging | GDB testing | Production |

## Best Practices

1. **Default to `gdb-test`** for hardware debugging
   - Good balance of debuggability and performance
   - Works well with real hardware timing

2. **Use `dev` only when needed**
   - When you need to inspect every variable
   - For algorithm debugging (not hardware)
   - When timing doesn't matter

3. **Never debug `release` builds**
   - Rebuild with `gdb-test` profile first
   - Only use release for final validation

4. **Keep test features minimal**
   - Enable only the tests you're debugging
   - Reduces binary size and link time

5. **Use automation when possible**
   - `./scripts/run_gdb_tests.sh --build` handles everything
   - Consistent, reproducible test environment

## See Also

- [GDB Test Scripts README](../scripts/README_GDB_TESTS.md)
- [Main README](../README.md)
- [Cargo Profiles Documentation](https://doc.rust-lang.org/cargo/reference/profiles.html)
