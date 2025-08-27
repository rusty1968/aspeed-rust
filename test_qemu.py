#!/usr/bin/env python3
# Licensed under the Apache-2.0 license

"""
ASPEED QEMU Test Script
This script builds and tests the ASPEED application using QEMU
"""

import argparse
import os
import subprocess
import sys
import signal
import time
from datetime import datetime
from pathlib import Path

# Colors for output
class Colors:
    RED = '\033[0;31m'
    GREEN = '\033[0;32m'
    YELLOW = '\033[1;33m'
    NC = '\033[0m'  # No Color

# Configuration
QEMU_MACHINE = "ast1030-evb"
TARGET = "thumbv7em-none-eabihf"
QEMU_CMD = "/home/ferro/qemu/bin/qemu-system-arm"
TIMEOUT_SECONDS = 60

def print_step(message):
    print(f"{Colors.GREEN}[STEP]{Colors.NC} {message}")

def print_warning(message):
    print(f"{Colors.YELLOW}[WARN]{Colors.NC} {message}")

def print_error(message):
    print(f"{Colors.RED}[ERROR]{Colors.NC} {message}")

def check_qemu_exists():
    """Check if QEMU binary exists and is executable"""
    if not os.path.isfile(QEMU_CMD) or not os.access(QEMU_CMD, os.X_OK):
        print_error(f"qemu-system-arm not found at {QEMU_CMD}")
        print_warning("To build QEMU with ASPEED support:")
        print("  cd /home/ferro/qemu")
        print("  mkdir build && cd build")
        print("  ../configure --target-list=arm-softmmu")
        print("  make -j 4")
        return False
    return True

def build_project(build_mode):
    """Build the project using cargo xtask"""
    print_step(f"Building project in {build_mode} mode...")
    
    try:
        if build_mode == "release":
            subprocess.run(["cargo", "xtask", "build", "--release"], 
                         check=True, cwd="/home/ferro/git/aspeed-rust")
            binary_path = f"target/{TARGET}/release/aspeed-ddk"
        else:
            subprocess.run(["cargo", "xtask", "build"], 
                         check=True, cwd="/home/ferro/git/aspeed-rust")
            binary_path = f"target/{TARGET}/debug/aspeed-ddk"
        
        full_binary_path = f"/home/ferro/git/aspeed-rust/{binary_path}"
        
        if not os.path.isfile(full_binary_path):
            print_error(f"Binary not found at {full_binary_path}")
            return None
            
        print_step(f"Binary built successfully: {full_binary_path}")
        return full_binary_path
        
    except subprocess.CalledProcessError as e:
        print_error(f"Build failed with exit code {e.returncode}")
        return None

def run_qemu_with_timeout(binary_path, machine, capture_output, output_file, timeout_sec):
    """Run QEMU with timeout and optional output capture"""
    cmd = [QEMU_CMD, "-M", machine, "-nographic", "-kernel", binary_path]
    
    print_step(f"Running QEMU with machine '{machine}' (timeout: {timeout_sec}s)...")
    if capture_output and output_file:
        print(f"Output will be captured to: {output_file}")
    
    print(f"Command: {' '.join(cmd)}")
    print("Press Ctrl+C to stop QEMU manually")
    print("-" * 40)
    
    # Start QEMU process
    try:
        if capture_output and output_file:
            # Use tee to capture output while displaying it
            tee_cmd = ["tee", output_file]
            
            qemu_process = subprocess.Popen(cmd, 
                                          stdout=subprocess.PIPE, 
                                          stderr=subprocess.STDOUT, 
                                          universal_newlines=True,
                                          bufsize=1)
            
            tee_process = subprocess.Popen(tee_cmd,
                                         stdin=qemu_process.stdout,
                                         stdout=sys.stdout,
                                         stderr=sys.stderr,
                                         universal_newlines=True)
            
            qemu_process.stdout.close()  # Allow qemu_process to receive a SIGPIPE if tee_process exits
            
            # Wait for timeout
            try:
                tee_process.wait(timeout=timeout_sec)
            except subprocess.TimeoutExpired:
                print_warning(f"\nQEMU execution timed out after {timeout_sec} seconds")
                print_step("Terminating QEMU process...")
                
                # Terminate processes
                qemu_process.terminate()
                tee_process.terminate()
                
                # Wait a bit for graceful termination
                time.sleep(1)
                
                # Force kill if still running
                try:
                    qemu_process.kill()
                    tee_process.kill()
                except:
                    pass
                
                print_step(f"Output captured in: {output_file}")
                return True
            
            # If we get here, the process completed before timeout
            qemu_process.wait()
            
        else:
            # Run without capture
            qemu_process = subprocess.Popen(cmd,
                                          stdout=sys.stdout,
                                          stderr=sys.stderr,
                                          universal_newlines=True)
            
            # Wait for timeout
            try:
                qemu_process.wait(timeout=timeout_sec)
            except subprocess.TimeoutExpired:
                print_warning(f"\nQEMU execution timed out after {timeout_sec} seconds")
                print_step("Terminating QEMU process...")
                
                # Terminate process
                qemu_process.terminate()
                
                # Wait a bit for graceful termination
                time.sleep(1)
                
                # Force kill if still running
                try:
                    qemu_process.kill()
                except:
                    pass
                
                return True
        
        print_step("QEMU process completed successfully!")
        return True
        
    except KeyboardInterrupt:
        print_warning("\nReceived interrupt signal (Ctrl+C)")
        print_step("Terminating QEMU process...")
        try:
            qemu_process.terminate()
            if capture_output and output_file:
                tee_process.terminate()
        except:
            pass
        return True
        
    except Exception as e:
        print_error(f"Error running QEMU: {e}")
        return False

def main():
    parser = argparse.ArgumentParser(description="ASPEED QEMU Test Script")
    parser.add_argument("--release", action="store_true",
                       help="Build in release mode (default: debug)")
    parser.add_argument("--machine", default=QEMU_MACHINE,
                       help=f"Set QEMU machine type (default: {QEMU_MACHINE})")
    parser.add_argument("--output", "-o", metavar="FILE",
                       help="Capture QEMU output to file")
    parser.add_argument("--timeout", type=int, default=TIMEOUT_SECONDS,
                       help=f"Set timeout in seconds (default: {TIMEOUT_SECONDS})")
    
    args = parser.parse_args()
    
    print_step("Starting ASPEED QEMU test...")
    
    # Check QEMU availability
    if not check_qemu_exists():
        return 1
    
    print_step(f"Using QEMU: {QEMU_CMD}")
    
    # Build project
    build_mode = "release" if args.release else "debug"
    binary_path = build_project(build_mode)
    if not binary_path:
        return 1
    
    # Set output file
    output_file = None
    capture_output = False
    if args.output:
        output_file = args.output
        capture_output = True
    elif args.output == "":  # --output without argument
        timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
        output_file = f"qemu_output_{timestamp}.log"
        capture_output = True
    
    # Run QEMU
    success = run_qemu_with_timeout(binary_path, args.machine, 
                                   capture_output, output_file, args.timeout)
    
    # Show output file location if captured
    if capture_output and output_file and os.path.isfile(output_file):
        print_step(f"Output captured in: {output_file}")
        print(f"To view the output: cat {output_file}")
    
    if success:
        print_step("QEMU test completed successfully!")
        return 0
    else:
        return 1

if __name__ == "__main__":
    sys.exit(main())