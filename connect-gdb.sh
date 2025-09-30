#!/bin/bash
# Licensed under the Apache-2.0 license

set -e

echo "Connecting GDB to QEMU on port 1234..."

# Create a GDB command file
cat > /tmp/gdb_commands.txt <<EOF
target remote :1234
load
break main
continue
EOF

gdb-multiarch -x /tmp/gdb_commands.txt target/thumbv7em-none-eabihf/debug/aspeed-ddk

# Cleanup
rm -f /tmp/gdb_commands.txt
