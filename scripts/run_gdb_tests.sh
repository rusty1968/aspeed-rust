#!/bin/bash
# Licensed under the Apache-2.0 license

# Shell wrapper for GDB test monitoring
# This script starts JLink GDB server and runs the Python monitor

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

# Default values
BINARY="${PROJECT_ROOT}/target/thumbv7em-none-eabihf/gdb-test/aspeed-ddk"
TEST_SUITE="all"
GDB_PORT=2331
TIMEOUT=120
DEVICE="cortex-m4"
INTERFACE="swd"
BUILD_PROFILE="gdb-test"
AUTO_BUILD=false

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_usage() {
    cat << EOF
Usage: $0 [OPTIONS]

Monitor ASPEED DDK functional tests via GDB

OPTIONS:
    -b, --binary PATH       Path to test binary (default: gdb-test profile build)
    -s, --suite SUITE       Test suite: all|hmac|hash|rsa|ecdsa (default: all)
    -t, --timeout SECONDS   Timeout in seconds (default: 120)
    -p, --port PORT         GDB server port (default: 2331)
    -d, --device DEVICE     Target device (default: cortex-m4)
    -i, --interface IFACE   Debug interface (default: swd)
    --profile PROFILE       Build profile: gdb-test|dev|debug|release (default: gdb-test)
    --build                 Automatically build binary before testing
    --no-jlink              Don't start JLink server (assume already running)
    -h, --help              Show this help message

PROFILES:
    gdb-test    Optimized for GDB debugging (minimal opt, full debug info)
    dev/debug   No optimizations, full debug info
    release     Optimized for size, no debug info

EXAMPLES:
    # Run all tests with default settings (auto-builds with gdb-test profile)
    $0 --build

    # Run only HMAC tests with automatic build
    $0 --build --suite hmac

    # Use existing debug binary
    $0 --binary target/thumbv7em-none-eabihf/debug/aspeed-ddk

    # Use release binary (not recommended for GDB)
    $0 --profile release --build --suite hash

    # Connect to existing JLink server
    $0 --no-jlink

EOF
}

# Parse arguments
START_JLINK=true
while [[ $# -gt 0 ]]; do
    case $1 in
        -b|--binary)
            BINARY="$2"
            shift 2
            ;;
        -s|--suite)
            TEST_SUITE="$2"
            shift 2
            ;;
        -t|--timeout)
            TIMEOUT="$2"
            shift 2
            ;;
        -p|--port)
            GDB_PORT="$2"
            shift 2
            ;;
        -d|--device)
            DEVICE="$2"
            shift 2
            ;;
        -i|--interface)
            INTERFACE="$2"
            shift 2
            ;;
        --profile)
            BUILD_PROFILE="$2"
            # Update binary path based on profile
            case "$BUILD_PROFILE" in
                gdb-test)
                    BINARY="${PROJECT_ROOT}/target/thumbv7em-none-eabihf/gdb-test/aspeed-ddk"
                    ;;
                dev|debug)
                    BINARY="${PROJECT_ROOT}/target/thumbv7em-none-eabihf/debug/aspeed-ddk"
                    ;;
                release)
                    BINARY="${PROJECT_ROOT}/target/thumbv7em-none-eabihf/release/aspeed-ddk"
                    ;;
                *)
                    echo -e "${RED}Error: Unknown profile $BUILD_PROFILE${NC}"
                    exit 1
                    ;;
            esac
            shift 2
            ;;
        --build)
            AUTO_BUILD=true
            shift
            ;;
        --no-jlink)
            START_JLINK=false
            shift
            ;;
        -h|--help)
            print_usage
            exit 0
            ;;
        *)
            echo -e "${RED}Error: Unknown option $1${NC}"
            print_usage
            exit 1
            ;;
    esac
done

# Auto-build if requested
if [ "$AUTO_BUILD" = true ]; then
    echo -e "${BLUE}[*]${NC} Building binary with profile: $BUILD_PROFILE"
    
    # Determine cargo build flags
    CARGO_FLAGS="--target thumbv7em-none-eabihf"
    
    # Add test features based on suite
    case "$TEST_SUITE" in
        all)
            CARGO_FLAGS="$CARGO_FLAGS --features test-hmac,test-hash,test-rsa,test-ecdsa"
            ;;
        hmac)
            CARGO_FLAGS="$CARGO_FLAGS --features test-hmac"
            ;;
        hash)
            CARGO_FLAGS="$CARGO_FLAGS --features test-hash"
            ;;
        rsa)
            CARGO_FLAGS="$CARGO_FLAGS --features test-rsa"
            ;;
        ecdsa)
            CARGO_FLAGS="$CARGO_FLAGS --features test-ecdsa"
            ;;
    esac
    
    # Add profile flag
    if [ "$BUILD_PROFILE" = "gdb-test" ]; then
        CARGO_FLAGS="$CARGO_FLAGS --profile gdb-test"
    elif [ "$BUILD_PROFILE" = "release" ]; then
        CARGO_FLAGS="$CARGO_FLAGS --release"
    fi
    
    echo -e "${BLUE}[*]${NC} Running: cargo build $CARGO_FLAGS"
    
    cd "$PROJECT_ROOT"
    if ! cargo build $CARGO_FLAGS; then
        echo -e "${RED}Error: Build failed${NC}"
        exit 1
    fi
    
    echo -e "${GREEN}[✓]${NC} Build completed successfully"
fi

# Check if binary exists
if [ ! -f "$BINARY" ]; then
    echo -e "${RED}Error: Binary not found: $BINARY${NC}"
    echo ""
    echo "You can:"
    echo "  1. Build with: $0 --build --suite $TEST_SUITE"
    echo "  2. Build manually: cargo build --profile $BUILD_PROFILE --target thumbv7em-none-eabihf --features test-$TEST_SUITE"
    echo "  3. Specify a different binary with: --binary <path>"
    exit 1
fi

# Check for required tools
if ! command -v gdb-multiarch &> /dev/null; then
    echo -e "${RED}Error: gdb-multiarch not found${NC}"
    echo "Install with: sudo apt-get install gdb-multiarch"
    exit 1
fi

if [ "$START_JLINK" = true ]; then
    if ! command -v JLinkGDBServer &> /dev/null; then
        echo -e "${YELLOW}Warning: JLinkGDBServer not found${NC}"
        echo "JLink tools are not installed. You can:"
        echo "  1. Install JLink from: https://www.segger.com/downloads/jlink/"
        echo "  2. Use QEMU instead (not yet supported by this script)"
        echo "  3. Run with --no-jlink if server is already running"
        exit 1
    fi
fi

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}ASPEED DDK GDB Test Monitor${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""
echo -e "${BLUE}Configuration:${NC}"
echo "  Binary:     $BINARY"
echo "  Profile:    $BUILD_PROFILE"
echo "  Test Suite: $TEST_SUITE"
echo "  Timeout:    ${TIMEOUT}s"
echo "  GDB Port:   $GDB_PORT"
echo "  Device:     $DEVICE"
echo "  Interface:  $INTERFACE"
echo ""

# Start JLink GDB server if requested
JLINK_PID=""
if [ "$START_JLINK" = true ]; then
    echo -e "${BLUE}[*]${NC} Starting JLink GDB Server..."
    
    # Kill any existing JLink server
    pkill -f JLinkGDBServer || true
    sleep 1
    
    # Start JLink GDB server in background
    JLinkGDBServer \
        -device "$DEVICE" \
        -if "$INTERFACE" \
        -port "$GDB_PORT" \
        -nohalt \
        -noir \
        > /tmp/jlink_gdb_server.log 2>&1 &
    
    JLINK_PID=$!
    
    # Wait for server to start
    echo -e "${BLUE}[*]${NC} Waiting for GDB server to start..."
    sleep 2
    
    if ! ps -p $JLINK_PID > /dev/null; then
        echo -e "${RED}Error: Failed to start JLink GDB Server${NC}"
        echo "Check /tmp/jlink_gdb_server.log for details"
        exit 1
    fi
    
    echo -e "${GREEN}[✓]${NC} JLink GDB Server started (PID: $JLINK_PID)"
else
    echo -e "${YELLOW}[!]${NC} Assuming JLink GDB Server is already running"
fi

# Cleanup function
cleanup() {
    if [ ! -z "$JLINK_PID" ] && ps -p $JLINK_PID > /dev/null; then
        echo -e "\n${BLUE}[*]${NC} Stopping JLink GDB Server..."
        kill $JLINK_PID 2>/dev/null || true
        wait $JLINK_PID 2>/dev/null || true
    fi
}

# Set trap to cleanup on exit
trap cleanup EXIT INT TERM

# Run the Python monitor via GDB
echo -e "\n${BLUE}[*]${NC} Starting GDB test monitor..."
echo ""

gdb-multiarch \
    --batch \
    --nx \
    -ex "python sys.path.insert(0, '${SCRIPT_DIR}')" \
    -ex "python exec(open('${SCRIPT_DIR}/gdb_test_monitor.py').read())" \
    --args "$BINARY" --suite "$TEST_SUITE" --timeout "$TIMEOUT" --port "$GDB_PORT"

EXIT_CODE=$?

# Report results
echo ""
if [ $EXIT_CODE -eq 0 ]; then
    echo -e "${GREEN}========================================${NC}"
    echo -e "${GREEN}Test monitoring completed successfully${NC}"
    echo -e "${GREEN}========================================${NC}"
else
    echo -e "${RED}========================================${NC}"
    echo -e "${RED}Test monitoring detected failures${NC}"
    echo -e "${RED}========================================${NC}"
fi

exit $EXIT_CODE
