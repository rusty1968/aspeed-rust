// Licensed under the Apache-2.0 license

use crate::digest::multi_context::MultiContextProvider;
use crate::digest::traits::HaceContextProvider;
use crate::uart::UartController;
use embedded_io::Write;

pub fn run_multi_context_tests(uart: &mut UartController) {
    writeln!(uart, "\r\n=== Multi-Context Provider Tests ===\r").unwrap();

    test_allocate_sessions(uart);
    test_release_and_reuse(uart);
    test_active_session(uart);
    test_is_session_allocated(uart);
    test_context_isolation(uart);

    writeln!(uart, "\r\n=== All Multi-Context Tests Passed ===\r").unwrap();
}

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
