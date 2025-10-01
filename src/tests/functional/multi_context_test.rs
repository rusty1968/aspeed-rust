// Licensed under the Apache-2.0 license

use crate::digest::hace_controller::HaceController;
use crate::digest::multi_context::MultiContextProvider;
use crate::digest::session::SessionManager;
use crate::digest::traits::HaceContextProvider;
use crate::uart::UartController;
use embedded_io::Write;

pub fn run_multi_context_tests(uart: &mut UartController, hace_controller: HaceController) {
    writeln!(uart, "\r\n=== Multi-Context Provider Tests ===\r").unwrap();

    test_allocate_sessions(uart);
    test_release_and_reuse(uart);
    test_active_session(uart);
    test_is_session_allocated(uart);
    test_context_isolation(uart);
    test_session_manager_basic(uart, hace_controller);

    writeln!(uart, "\r\n=== All Multi-Context Tests Passed ===\r").unwrap();
}

// ============================================================================
// Low-level MultiContextProvider tests
// ============================================================================

fn test_allocate_sessions(uart: &mut UartController) {
    write!(uart, "Testing session allocation... ").unwrap();

    let mut provider = MultiContextProvider::new(4).unwrap();

    let s1 = provider.allocate_session().unwrap();
    let s2 = provider.allocate_session().unwrap();
    let s3 = provider.allocate_session().unwrap();
    let s4 = provider.allocate_session().unwrap();

    assert_eq!(s1, 0);
    assert_eq!(s2, 1);
    assert_eq!(s3, 2);
    assert_eq!(s4, 3);

    // Should fail - all slots allocated
    assert!(provider.allocate_session().is_err());

    writeln!(uart, "PASSED\r").unwrap();
}

fn test_release_and_reuse(uart: &mut UartController) {
    write!(uart, "Testing session release and reuse... ").unwrap();

    let mut provider = MultiContextProvider::new(2).unwrap();

    let s1 = provider.allocate_session().unwrap();
    assert_eq!(s1, 0);

    provider.release_session(s1);

    let s2 = provider.allocate_session().unwrap();
    assert_eq!(s2, 0); // Should reuse slot 0

    writeln!(uart, "PASSED\r").unwrap();
}

fn test_active_session(uart: &mut UartController) {
    write!(uart, "Testing active session switching... ").unwrap();

    let mut provider = MultiContextProvider::new(4).unwrap();

    let s1 = provider.allocate_session().unwrap();
    let s2 = provider.allocate_session().unwrap();

    provider.set_active_session(s1);
    assert_eq!(provider.active_session(), s1);

    provider.set_active_session(s2);
    assert_eq!(provider.active_session(), s2);

    writeln!(uart, "PASSED\r").unwrap();
}

fn test_is_session_allocated(uart: &mut UartController) {
    write!(uart, "Testing session allocation status... ").unwrap();

    let mut provider = MultiContextProvider::new(4).unwrap();

    let s1 = provider.allocate_session().unwrap();
    assert!(provider.is_session_allocated(s1));
    assert!(!provider.is_session_allocated(1));

    provider.release_session(s1);
    assert!(!provider.is_session_allocated(s1));

    writeln!(uart, "PASSED\r").unwrap();
}

fn test_context_isolation(uart: &mut UartController) {
    write!(uart, "Testing context isolation... ").unwrap();

    let mut provider = MultiContextProvider::new(4).unwrap();

    let s1 = provider.allocate_session().unwrap();
    let s2 = provider.allocate_session().unwrap();

    // Set some data in session 1
    provider.set_active_session(s1);
    {
        let ctx = provider.ctx_mut().unwrap();
        ctx.bufcnt = 42;
        ctx.buffer[0] = 0xAA;
        ctx.buffer[1] = 0xBB;
    }

    // Switch to session 2 and set different data
    provider.set_active_session(s2);
    {
        let ctx = provider.ctx_mut().unwrap();
        ctx.bufcnt = 99;
        ctx.buffer[0] = 0xCC;
        ctx.buffer[1] = 0xDD;
    }

    // Switch back to session 1 and verify data is preserved
    provider.set_active_session(s1);
    {
        let ctx = provider.ctx_mut().unwrap();
        assert_eq!(ctx.bufcnt, 42);
        assert_eq!(ctx.buffer[0], 0xAA);
        assert_eq!(ctx.buffer[1], 0xBB);
    }

    // Switch back to session 2 and verify its data
    provider.set_active_session(s2);
    {
        let ctx = provider.ctx_mut().unwrap();
        assert_eq!(ctx.bufcnt, 99);
        assert_eq!(ctx.buffer[0], 0xCC);
        assert_eq!(ctx.buffer[1], 0xDD);
    }

    writeln!(uart, "PASSED\r").unwrap();
}

// ============================================================================
// SessionManager API tests
// ============================================================================

fn test_session_manager_basic(uart: &mut UartController, hace_controller: HaceController) {
    write!(uart, "Testing SessionManager concurrent hashing... ").unwrap();

    // Create session manager supporting 3 concurrent sessions
    let mut manager = SessionManager::<3>::new(hace_controller.hace).unwrap();

    // Verify initial state
    assert_eq!(manager.active_count(), 0);
    assert_eq!(manager.max_sessions(), 3);

    // Test 1: Initialize and finalize single session
    {
        let session = manager.init_sha256().unwrap();
        assert_eq!(manager.active_count(), 1);

        let session = session.update(b"abc").unwrap();
        let (digest, _handle) = manager.finalize(session).unwrap();

        // SHA-256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        let expected: [u8; 32] = [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ];

        assert_eq!(digest.as_ref(), &expected);
        assert_eq!(manager.active_count(), 0); // Session released
    }

    // Test 2: Concurrent sessions with interleaved updates
    {
        // Start 3 concurrent sessions
        let mut s1 = manager.init_sha256().unwrap();
        let mut s2 = manager.init_sha256().unwrap();
        let mut s3 = manager.init_sha256().unwrap();

        assert_eq!(manager.active_count(), 3);

        // Interleave updates (this demonstrates automatic context switching)
        s1 = s1.update(b"abc").unwrap();
        s2 = s2.update(b"hello ").unwrap();
        s3 = s3.update(b"test").unwrap();
        s2 = s2.update(b"world").unwrap(); // Complete s2 data

        // Finalize in different order than created
        let (digest2, _) = manager.finalize(s2).unwrap();
        assert_eq!(manager.active_count(), 2);

        let (digest1, _) = manager.finalize(s1).unwrap();
        assert_eq!(manager.active_count(), 1);

        let (digest3, _) = manager.finalize(s3).unwrap();
        assert_eq!(manager.active_count(), 0);

        // Verify results
        let expected_abc: [u8; 32] = [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ];

        // SHA-256("hello world") = b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9
        let expected_hello: [u8; 32] = [
            0xb9, 0x4d, 0x27, 0xb9, 0x93, 0x4d, 0x3e, 0x08, 0xa5, 0x2e, 0x52, 0xd7, 0xda, 0x7d,
            0xab, 0xfa, 0xc4, 0x84, 0xef, 0xe3, 0x7a, 0x53, 0x80, 0xee, 0x90, 0x88, 0xf7, 0xac,
            0xe2, 0xef, 0xcd, 0xe9,
        ];

        // SHA-256("test") = 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08
        let expected_test: [u8; 32] = [
            0x9f, 0x86, 0xd0, 0x81, 0x88, 0x4c, 0x7d, 0x65, 0x9a, 0x2f, 0xea, 0xa0, 0xc5, 0x5a,
            0xd0, 0x15, 0xa3, 0xbf, 0x4f, 0x1b, 0x2b, 0x0b, 0x82, 0x2c, 0xd1, 0x5d, 0x6c, 0x15,
            0xb0, 0xf0, 0x0a, 0x08,
        ];

        assert_eq!(digest1.as_ref(), &expected_abc);
        assert_eq!(digest2.as_ref(), &expected_hello);
        assert_eq!(digest3.as_ref(), &expected_test);
    }

    // Test 3: Session limit enforcement
    {
        let s1 = manager.init_sha256().unwrap();
        let s2 = manager.init_sha256().unwrap();
        let s3 = manager.init_sha256().unwrap();

        // Fourth should fail (limit is 3)
        assert!(manager.init_sha256().is_err());

        // Release one session
        manager.finalize(s1).unwrap();

        // Now fourth should succeed
        let s4 = manager.init_sha256().unwrap();
        assert_eq!(manager.active_count(), 3);

        // Cleanup
        manager.finalize(s2).unwrap();
        manager.finalize(s3).unwrap();
        manager.finalize(s4).unwrap();
        assert_eq!(manager.active_count(), 0);
    }

    // Test 4: Cancel operation
    {
        let session = manager.init_sha256().unwrap();
        assert_eq!(manager.active_count(), 1);

        let session = session.update(b"data").unwrap();

        // Cancel instead of finalize
        manager.cancel(session).unwrap();
        assert_eq!(manager.active_count(), 0); // Session released
    }

    // Test 5: Multiple algorithms
    {
        let sha256_session = manager.init_sha256().unwrap();
        let sha384_session = manager.init_sha384().unwrap();
        let sha512_session = manager.init_sha512().unwrap();

        assert_eq!(manager.active_count(), 3);

        // Update each with same data
        let sha256_session = sha256_session.update(b"test").unwrap();
        let sha384_session = sha384_session.update(b"test").unwrap();
        let sha512_session = sha512_session.update(b"test").unwrap();

        // Finalize and verify different output sizes
        let (digest256, _) = manager.finalize(sha256_session).unwrap();
        let (digest384, _) = manager.finalize(sha384_session).unwrap();
        let (digest512, _) = manager.finalize(sha512_session).unwrap();

        assert_eq!(digest256.as_ref().len(), 32); // 256 bits / 8
        assert_eq!(digest384.as_ref().len(), 48); // 384 bits / 8
        assert_eq!(digest512.as_ref().len(), 64); // 512 bits / 8

        assert_eq!(manager.active_count(), 0);
    }

    writeln!(uart, "PASSED\r").unwrap();
}
