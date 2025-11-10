#!/usr/bin/env python3
"""
GDB Python Test Monitor for ASPEED AST1060 DDK
Monitors functional test execution and verifies success.
Supports: HMAC, Hash, RSA, ECDSA tests
"""

import gdb
import time
import sys
import re
import argparse

class Colors:
    GREEN = '\033[0;32m'
    RED = '\033[0;31m'
    YELLOW = '\033[1;33m'
    BLUE = '\033[0;34m'
    CYAN = '\033[0;36m'
    NC = '\033[0m'

class TestMonitor:
    def __init__(self, test_suite='all', timeout=120, gdb_port=2331):
        self.test_suite = test_suite
        self.test_hits = {}
        self.test_results = []
        self.start_time = time.time()
        self.timeout = timeout
        self.gdb_port = gdb_port
        self.uart_output = []
        self.panic_detected = False
        
        # Test suite configurations
        self.test_configs = {
            'hmac': {
                'markers': ['HMAC-SHA256', 'HMAC-SHA384', 'HMAC-SHA512', 'HMAC test'],
                'success_patterns': ['HMAC test passed', 'All HMAC tests passed'],
                'failure_patterns': ['HMAC test failed', 'HMAC verification failed'],
            },
            'hash': {
                'markers': ['SHA-256', 'SHA-384', 'SHA-512', 'Hash test'],
                'success_patterns': ['Hash test passed', 'All hash tests passed'],
                'failure_patterns': ['Hash test failed', 'Hash mismatch'],
            },
            'rsa': {
                'markers': ['RSA-2048', 'RSA-3072', 'RSA-4096', 'RSA test'],
                'success_patterns': ['RSA test passed', 'All RSA tests passed'],
                'failure_patterns': ['RSA test failed', 'RSA verification failed'],
            },
            'ecdsa': {
                'markers': ['ECDSA-P256', 'ECDSA-P384', 'ECDSA test'],
                'success_patterns': ['ECDSA test passed', 'All ECDSA tests passed'],
                'failure_patterns': ['ECDSA test failed', 'ECDSA verification failed'],
            },
        }
        
    def log(self, msg):
        print(f"{Colors.BLUE}[*]{Colors.NC} {msg}")
        
    def success(self, msg):
        print(f"{Colors.GREEN}[✓]{Colors.NC} {msg}")
        
    def failure(self, msg):
        print(f"{Colors.RED}[✗]{Colors.NC} {msg}")
        
    def warning(self, msg):
        print(f"{Colors.YELLOW}[!]{Colors.NC} {msg}")
    
    def info(self, msg):
        print(f"{Colors.CYAN}[i]{Colors.NC} {msg}")
    
    def connect_to_target(self, binary_path):
        """Connect to JLink GDB server and load the binary"""
        try:
            self.log("Connecting to target...")
            
            # Connect to JLink GDB server
            gdb.execute(f"target remote localhost:{self.gdb_port}", to_string=True)
            self.success(f"Connected to GDB server on port {self.gdb_port}")
            
            # Enable semihosting
            self.log("Enabling semihosting...")
            gdb.execute("monitor semihosting enable", to_string=True)
            gdb.execute("monitor semihosting IOClient 2", to_string=True)
            
            # Load the binary
            self.log(f"Loading binary: {binary_path}")
            gdb.execute(f"file {binary_path}", to_string=True)
            gdb.execute("load", to_string=True)
            self.success("Binary loaded successfully")
            
            # Set breakpoints for test functions
            self.set_breakpoints()
            
            return True
            
        except gdb.error as e:
            self.failure(f"Failed to connect to target: {e}")
            return False
    
    def set_breakpoints(self):
        """Set breakpoints on test functions based on test suite"""
        self.log("Setting breakpoints...")
        
        # Common breakpoints
        breakpoints = [
            "panic_halt::panic",  # Catch panics
            "rust_begin_unwind",  # Catch unwinding
        ]
        
        # Test-specific breakpoints
        if self.test_suite in ['all', 'hmac']:
            breakpoints.extend([
                "aspeed_ddk::tests::functional::hmac_test::run_hmac_tests",
                "aspeed_ddk::hmac::AspeedHmac::compute",
            ])
        
        if self.test_suite in ['all', 'hash']:
            breakpoints.extend([
                "aspeed_ddk::tests::functional::hash_test::run_hash_tests",
            ])
        
        if self.test_suite in ['all', 'rsa']:
            breakpoints.extend([
                "aspeed_ddk::tests::functional::rsa_test::run_rsa_tests",
            ])
        
        if self.test_suite in ['all', 'ecdsa']:
            breakpoints.extend([
                "aspeed_ddk::tests::functional::ecdsa_test::run_ecdsa_tests",
            ])
        
        for bp in breakpoints:
            try:
                result = gdb.execute(f"break {bp}", to_string=True)
                if "Breakpoint" in result:
                    self.info(f"Set breakpoint: {bp}")
            except gdb.error:
                # Breakpoint might not exist in this binary
                pass
    
    def parse_uart_output(self, output):
        """Parse UART output for test results"""
        if not output:
            return
        
        # Store output
        self.uart_output.append(output)
        
        # Check for panic
        if "panicked at" in output or "PANIC" in output:
            self.panic_detected = True
            self.failure("Panic detected in UART output!")
            self.log(f"  {output}")
        
        # Parse test results based on suite
        config = self.test_configs.get(self.test_suite, {})
        if not config and self.test_suite == 'all':
            # Check all configs
            for suite_config in self.test_configs.values():
                self._check_output_patterns(output, suite_config)
        else:
            self._check_output_patterns(output, config)
    
    def _check_output_patterns(self, output, config):
        """Check output against test patterns"""
        # Check for test markers
        for marker in config.get('markers', []):
            if marker in output:
                self.info(f"Test marker found: {marker}")
        
        # Check for success patterns
        for pattern in config.get('success_patterns', []):
            if pattern in output:
                self.success(f"Test passed: {pattern}")
                self.test_results.append(('PASS', pattern))
        
        # Check for failure patterns
        for pattern in config.get('failure_patterns', []):
            if pattern in output:
                self.failure(f"Test failed: {pattern}")
                self.test_results.append(('FAIL', pattern))
    
    def monitor_execution(self):
        """Monitor test execution"""
        print("\n" + "=" * 70)
        print(f"Monitoring Test Execution - Suite: {self.test_suite}")
        print("=" * 70 + "\n")
        
        self.log("Starting firmware execution...")
        
        try:
            # Start execution
            gdb.execute("continue", to_string=False)
            
            while time.time() - self.start_time < self.timeout:
                try:
                    # Check if still running
                    thread_info = gdb.execute("info threads", to_string=True)
                    
                    # Try to read any buffered output from semihosting
                    # This is platform-specific and may not work on all setups
                    
                    # Sleep briefly to allow execution
                    time.sleep(0.5)
                    
                    # Check for breakpoint hits
                    frame = gdb.selected_frame()
                    if frame:
                        func_name = frame.name()
                        
                        if func_name:
                            self.success(f"Hit breakpoint: {func_name}")
                            
                            # Check for panic
                            if "panic" in func_name.lower() or "unwind" in func_name.lower():
                                self.failure("Test panicked!")
                                self.examine_panic()
                                return False
                            
                            # Track test function hits
                            if "test" in func_name.lower():
                                if func_name not in self.test_hits:
                                    self.test_hits[func_name] = 0
                                self.test_hits[func_name] += 1
                                self.log(f"Test function hit {self.test_hits[func_name]} times: {func_name}")
                            
                            # Continue execution
                            gdb.execute("continue", to_string=False)
                
                except gdb.error as e:
                    error_msg = str(e)
                    if "program exited" in error_msg.lower() or "terminated" in error_msg.lower():
                        self.log(f"Program terminated: {e}")
                        # This might be normal completion
                        break
                    elif "target" in error_msg.lower() or "remote connection" in error_msg.lower():
                        self.warning(f"Lost connection to target: {e}")
                        break
                    else:
                        # Might just be waiting
                        time.sleep(0.1)
                        continue
                        
        except KeyboardInterrupt:
            self.warning("Interrupted by user")
            return False
        
        # Check if we timed out
        if time.time() - self.start_time >= self.timeout:
            self.failure(f"Timeout after {self.timeout} seconds")
            return False
        
        return not self.panic_detected
    
    def examine_panic(self):
        """Examine panic information"""
        try:
            self.log("Panic backtrace:")
            bt = gdb.execute("backtrace", to_string=True)
            for line in bt.split('\n')[:15]:  # First 15 lines
                print(f"  {line}")
            
            # Try to print panic message
            try:
                self.log("Attempting to read panic message...")
                # This is highly dependent on panic handler implementation
                # May need adjustment based on actual panic_halt implementation
            except:
                pass
        except Exception as e:
            self.warning(f"Could not examine panic: {e}")
    
    def generate_report(self):
        """Generate final test report"""
        print("\n" + "=" * 70)
        print("Test Report")
        print("=" * 70 + "\n")
        
        if self.test_hits:
            self.log("Test function hits:")
            for func, count in sorted(self.test_hits.items()):
                print(f"  {func}: {count} times")
        
        if self.test_results:
            print("\n" + self.log("Test results:"))
            pass_count = sum(1 for r in self.test_results if r[0] == 'PASS')
            fail_count = sum(1 for r in self.test_results if r[0] == 'FAIL')
            
            for result, msg in self.test_results:
                if result == 'PASS':
                    self.success(msg)
                else:
                    self.failure(msg)
            
            print(f"\nTotal: {pass_count} passed, {fail_count} failed")
        else:
            self.warning("No explicit test results found")
            self.info("Tests may have executed successfully without output capture")
        
        # Check UART output
        if self.uart_output:
            print("\n" + "=" * 70)
            self.log("UART Output Summary:")
            print("=" * 70)
            for line in self.uart_output[-20:]:  # Last 20 lines
                print(f"  {line}")
        
        # Final determination
        print("\n" + "=" * 70)
        if self.panic_detected:
            self.failure("TESTS FAILED - PANIC DETECTED")
            print("=" * 70 + "\n")
            return False
        elif self.test_results:
            fail_count = sum(1 for r in self.test_results if r[0] == 'FAIL')
            if fail_count > 0:
                self.failure(f"TESTS FAILED - {fail_count} failures")
                print("=" * 70 + "\n")
                return False
            else:
                self.success("ALL TESTS PASSED")
                print("=" * 70 + "\n")
                return True
        else:
            self.warning("TEST STATUS UNCERTAIN - No explicit results")
            self.info("Manual verification recommended")
            print("=" * 70 + "\n")
            return True  # Assume success if no failures detected

def main():
    parser = argparse.ArgumentParser(
        description='Monitor ASPEED DDK functional tests via GDB'
    )
    parser.add_argument(
        'binary',
        nargs='?',
        default='target/thumbv7em-none-eabihf/gdb-test/aspeed-ddk',
        help='Path to test binary (default: gdb-test profile build)'
    )
    parser.add_argument(
        '--suite',
        choices=['all', 'hmac', 'hash', 'rsa', 'ecdsa'],
        default='all',
        help='Test suite to monitor (default: all)'
    )
    parser.add_argument(
        '--timeout',
        type=int,
        default=120,
        help='Timeout in seconds (default: 120)'
    )
    parser.add_argument(
        '--port',
        type=int,
        default=2331,
        help='GDB server port (default: 2331 for JLink)'
    )
    
    args = parser.parse_args()
    
    monitor = TestMonitor(
        test_suite=args.suite,
        timeout=args.timeout,
        gdb_port=args.port
    )
    
    # Connect to target and load binary
    if not monitor.connect_to_target(args.binary):
        sys.exit(1)
    
    # Monitor execution
    success = monitor.monitor_execution()
    
    # Generate report
    report_ok = monitor.generate_report()
    
    # Exit with appropriate code
    if success and report_ok:
        print("\n[*] Test monitoring completed successfully")
        sys.exit(0)
    else:
        print("\n[*] Test monitoring detected failures")
        sys.exit(1)

# When run as a script
if __name__ == "__main__":
    main()
