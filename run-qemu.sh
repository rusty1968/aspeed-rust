#!/bin/bash
# Licensed under the Apache-2.0 license

set -e

echo "Building aspeed-ddk..."
cargo build

echo "Starting QEMU with GDB server on port 1234..."
qemu-system-arm -M ast1030-evb -nographic -s -S -kernel target/thumbv7em-none-eabihf/debug/aspeed-ddk
