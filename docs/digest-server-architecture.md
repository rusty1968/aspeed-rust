# Digest Server Layered Architecture

**Date**: 2025-09-30
**Status**: Architecture Reference Document

## Overview

This document provides a comprehensive layered architecture diagram for the Hubris digest server, showing how components interact from hardware up to the IPC layer.

## Full Stack Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         IPC LAYER (Hubris Idol)                             │
│  ┌───────────────────────────────────────────────────────────────────────┐  │
│  │  Client Tasks (SPDM Responder, Attestation, TLS, SSH, etc.)          │  │
│  │  - Request digest operations via IPC                                  │  │
│  │  - Receive session IDs and digest results                             │  │
│  └─────────────────────────────────┬─────────────────────────────────────┘  │
└────────────────────────────────────┼────────────────────────────────────────┘
                                     │ IPC calls (init/update/finalize)
                                     ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                    APPLICATION LAYER (Digest Server)                        │
│  ┌───────────────────────────────────────────────────────────────────────┐  │
│  │  DigestServerImpl (Idol Server)                                       │  │
│  │  - Handles IPC requests                                               │  │
│  │  - Manages session state                                              │  │
│  │  - Validates inputs                                                   │  │
│  │  - Routes to appropriate algorithm                                    │  │
│  └─────────────────────────────────┬─────────────────────────────────────┘  │
│                                     │                                        │
│  State Storage:                     │                                        │
│  ┌──────────────────────────────────▼──────────────────────────────────┐   │
│  │  SessionManager<8>                                                   │   │
│  │  - manager: SessionManager<8>                                        │   │
│  │  - sessions: [Option<ActiveDigest>; 8]                               │   │
│  │                                                                        │   │
│  │  enum ActiveDigest {                                                  │   │
│  │    Sha256(SessionDigest<Sha2_256>),                                  │   │
│  │    Sha384(SessionDigest<Sha2_384>),                                  │   │
│  │    Sha512(SessionDigest<Sha2_512>),                                  │   │
│  │  }                                                                    │   │
│  └──────────────────────────────────┬───────────────────────────────────┘   │
└────────────────────────────────────┼────────────────────────────────────────┘
                                     │ High-level API
                                     ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│               SESSION MANAGEMENT LAYER (aspeed-ddk::digest::session)        │
│  ┌───────────────────────────────────────────────────────────────────────┐  │
│  │  SessionManager<N: usize>                                             │  │
│  │  ┌─────────────────────────────────────────────────────────────────┐ │  │
│  │  │  API Methods:                                                    │ │  │
│  │  │  - init_sha256() -> SessionDigest<Sha2_256>                     │ │  │
│  │  │  - init_sha384() -> SessionDigest<Sha2_384>                     │ │  │
│  │  │  - init_sha512() -> SessionDigest<Sha2_512>                     │ │  │
│  │  │  - finalize<T>(digest) -> (T::Digest, SessionHandle)           │ │  │
│  │  │  - cancel<T>(digest)                                            │ │  │
│  │  │  - active_count(), max_sessions(), is_valid()                  │ │  │
│  │  └─────────────────────────────────────────────────────────────────┘ │  │
│  │                                                                        │  │
│  │  Internal State:                                                       │  │
│  │  - controller: Option<HaceController<MultiContextProvider>>           │  │
│  │  - sessions: [SessionSlot; N]                                         │  │
│  │  - next_id: u32 (wrapping session ID counter)                         │  │
│  └──────────────────────────────────┬─────────────────────────────────────┘  │
│                                     │                                        │
│  ┌──────────────────────────────────▼──────────────────────────────────┐   │
│  │  SessionDigest<T: DigestAlgorithm>                                   │   │
│  │  - context: OwnedDigestContext<T, MultiContextProvider>             │   │
│  │  - provider_session_id: usize                                        │   │
│  │  - manager_session_id: u32                                           │   │
│  │  - slot: usize                                                       │   │
│  │                                                                        │   │
│  │  Methods:                                                             │   │
│  │  - update(data) -> Self  [auto-activates session]                   │   │
│  │  - handle() -> SessionHandle<T>                                      │   │
│  └──────────────────────────────────┬───────────────────────────────────┘   │
└────────────────────────────────────┼────────────────────────────────────────┘
                                     │ Owned API + Session tracking
                                     ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│              DIGEST API LAYER (aspeed-ddk::digest)                          │
│  ┌───────────────────────────────────────────────────────────────────────┐  │
│  │  hash_owned.rs - Owned (Move-Based) API                              │  │
│  │  ┌─────────────────────────────────────────────────────────────────┐ │  │
│  │  │  OwnedDigestContext<T, P>                                        │ │  │
│  │  │  - controller: HaceController<P>                                │ │  │
│  │  │  - _phantom: PhantomData<T>                                     │ │  │
│  │  │                                                                  │ │  │
│  │  │  Traits: DigestInit, DigestOp                                   │ │  │
│  │  │  - init(controller, algo) -> Context                            │ │  │
│  │  │  - update(self, data) -> Self                                   │ │  │
│  │  │  - finalize(self) -> (Digest, Controller)                       │ │  │
│  │  │  - cancel(self) -> Controller                                   │ │  │
│  │  │  - controller_mut() -> &mut Controller  [for session mgmt]     │ │  │
│  │  └─────────────────────────────────────────────────────────────────┘ │  │
│  └──────────────────────────────────┬─────────────────────────────────────┘  │
│                                     │                                        │
│  ┌──────────────────────────────────▼──────────────────────────────────┐   │
│  │  hash.rs - Scoped (Borrowed) API                                    │   │
│  │  ┌─────────────────────────────────────────────────────────────────┐│   │
│  │  │  OpContextImpl<'a, A, P>                                        ││   │
│  │  │  - controller: &'a mut HaceController<P>                        ││   │
│  │  │  - _phantom: PhantomData<A>                                     ││   │
│  │  │                                                                  ││   │
│  │  │  Traits: DigestInit, DigestOp                                   ││   │
│  │  │  - init(&mut controller) -> OpContext<'_>                       ││   │
│  │  │  - update(&mut self, data)                                      ││   │
│  │  │  - finalize(self) -> Digest                                     ││   │
│  │  │                                                                  ││   │
│  │  │  Note: Cannot cross IPC boundaries (lifetime 'a)                ││   │
│  │  └─────────────────────────────────────────────────────────────────┘│   │
│  └──────────────────────────────────┬───────────────────────────────────┘   │
└────────────────────────────────────┼────────────────────────────────────────┘
                                     │ Controller abstraction
                                     ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│          MULTI-CONTEXT LAYER (aspeed-ddk::digest::multi_context)            │
│  ┌───────────────────────────────────────────────────────────────────────┐  │
│  │  HaceController<P: HaceContextProvider>                               │  │
│  │  - hace: Hace (hardware peripheral)                                   │  │
│  │  - algo: HashAlgo (current algorithm)                                 │  │
│  │  - provider: P (context provider)                                     │  │
│  │                                                                        │  │
│  │  Methods:                                                             │  │
│  │  - new(hace) -> HaceController<SingleContextProvider>                │  │
│  │  - with_provider(hace, provider) -> HaceController<P>                │  │
│  │  - provider_mut() -> &mut P                                          │  │
│  │  - ctx_mut_unchecked() -> &mut AspeedHashContext                     │  │
│  │  - start_hash_operation(), copy_iv_to_digest(), etc.                 │  │
│  └──────────────────────────────────┬─────────────────────────────────────┘  │
│                                     │                                        │
│         Provider Implementations:   │                                        │
│  ┌──────────────────────────────────┴──────────────────────────────────┐   │
│  │  trait HaceContextProvider                                           │   │
│  │    fn ctx_mut(&mut self) -> Result<&mut AspeedHashContext, Error>   │   │
│  └──────────────┬────────────────────────────┬──────────────────────────┘   │
│                 │                            │                               │
│  ┌──────────────▼──────────┐  ┌─────────────▼────────────────────────┐     │
│  │ SingleContextProvider   │  │  MultiContextProvider                 │     │
│  │ (Zero-cost default)     │  │  (Session management)                │     │
│  │                         │  │                                       │     │
│  │ - Direct access to      │  │  - contexts: [MaybeUninit<Ctx>; N]   │     │
│  │   shared_hash_ctx()     │  │  - allocated: [bool; N]              │     │
│  │ - Always succeeds       │  │  - active_id: usize                  │     │
│  │ - Zero runtime overhead │  │  - last_loaded: Option<usize>        │     │
│  │                         │  │                                       │     │
│  │ Use case:               │  │  Methods:                             │     │
│  │ - Single session        │  │  - allocate_session() -> usize       │     │
│  │ - Simple applications   │  │  - release_session(id)               │     │
│  │                         │  │  - set_active_session(id)            │     │
│  │                         │  │  - ctx_mut() [with lazy switching]   │     │
│  │                         │  │                                       │     │
│  │                         │  │  Context Switching:                   │     │
│  │                         │  │  - save_hw_to_slot(id) [~732 bytes]  │     │
│  │                         │  │  - load_slot_to_hw(id) [~732 bytes]  │     │
│  │                         │  │  - Lazy: only switches when needed   │     │
│  │                         │  │                                       │     │
│  │                         │  │  Use case:                            │     │
│  │                         │  │  - Multi-session IPC servers         │     │
│  │                         │  │  - Concurrent hash operations        │     │
│  └─────────────────────────┘  └───────────────┬──────────────────────┘     │
└────────────────────────────────────────────────┼────────────────────────────┘
                                                 │ Context access
                                                 ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│              HARDWARE ABSTRACTION LAYER (hace_controller)                   │
│  ┌───────────────────────────────────────────────────────────────────────┐  │
│  │  AspeedHashContext (in .ram_nc section)                              │  │
│  │  ┌─────────────────────────────────────────────────────────────────┐ │  │
│  │  │  struct AspeedHashContext {                                      │ │  │
│  │  │    sg: [AspeedSg; 2],           // Scatter-gather descriptors   │ │  │
│  │  │    digest: [u8; 64],            // Current hash state (IV/digest)│ │  │
│  │  │    method: u32,                 // Algorithm + flags            │ │  │
│  │  │    block_size: u32,             // Block size (64/128 bytes)    │ │  │
│  │  │    digcnt: [u64; 2],            // Total bytes processed        │ │  │
│  │  │    bufcnt: u32,                 // Bytes in buffer              │ │  │
│  │  │    buffer: [u8; 256],           // Pending data buffer          │ │  │
│  │  │    iv_size: u8,                 // IV size for this algorithm   │ │  │
│  │  │                                                                  │ │  │
│  │  │    // HMAC fields:                                              │ │  │
│  │  │    key: [u8; 128],              // HMAC key                     │ │  │
│  │  │    key_len: u32,                // Key length                   │ │  │
│  │  │    ipad: [u8; 128],             // HMAC inner padding           │ │  │
│  │  │    opad: [u8; 128],             // HMAC outer padding           │ │  │
│  │  │  }                                                               │ │  │
│  │  │  Total: ~732 bytes per context                                  │ │  │
│  │  └─────────────────────────────────────────────────────────────────┘ │  │
│  │                                                                        │  │
│  │  Global Instance:                                                      │  │
│  │  ┌─────────────────────────────────────────────────────────────────┐ │  │
│  │  │  #[link_section = ".ram_nc"]                                    │ │  │
│  │  │  static SHARED_HASH_CTX: SectionPlacedContext                   │ │  │
│  │  │                                                                  │ │  │
│  │  │  fn shared_hash_ctx() -> *mut AspeedHashContext                │ │  │
│  │  └─────────────────────────────────────────────────────────────────┘ │  │
│  └──────────────────────────────────┬─────────────────────────────────────┘  │
└────────────────────────────────────┼────────────────────────────────────────┘
                                     │ MMIO register access
                                     ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                   HARDWARE LAYER (ASPEED AST1060)                           │
│  ┌───────────────────────────────────────────────────────────────────────┐  │
│  │  HACE (Hash and Crypto Engine) Peripheral                            │  │
│  │  ┌─────────────────────────────────────────────────────────────────┐ │  │
│  │  │  Base Address: 0x7e6d_0000                                       │ │  │
│  │  │                                                                  │ │  │
│  │  │  Registers:                                                      │ │  │
│  │  │  - SRC        [0x00]: Source address (scatter-gather pointer)  │ │  │
│  │  │  - DEST       [0x04]: Destination address (digest output)      │ │  │
│  │  │  - CONTEXT    [0x08]: Context address (state storage)          │ │  │
│  │  │  - DATA_LEN   [0x0C]: Data length                              │ │  │
│  │  │  - CMD        [0x10]: Command/control register                 │ │  │
│  │  │  - STATUS     [0x1C]: Status register                          │ │  │
│  │  │  - HASH_SRC   [0x20]: Hash source pointer                      │ │  │
│  │  │  - HASH_DIGEST_BUFF [0x24]: Digest buffer address             │ │  │
│  │  │  - HASH_KEY_BUFF [0x28]: Key buffer address (for HMAC)        │ │  │
│  │  │  - HASH_DATA_LEN [0x2C]: Hash data length                     │ │  │
│  │  │  - HASH_CMD   [0x30]: Hash command register                   │ │  │
│  │  │                                                                  │ │  │
│  │  │  Supported Algorithms:                                          │ │  │
│  │  │  - SHA-1, SHA-224, SHA-256                                     │ │  │
│  │  │  - SHA-384, SHA-512, SHA-512/224, SHA-512/256                 │ │  │
│  │  │  - HMAC variants of above                                      │ │  │
│  │  │  - MD5 (legacy)                                                │ │  │
│  │  │                                                                  │ │  │
│  │  │  Features:                                                      │ │  │
│  │  │  - DMA-based scatter-gather operations                         │ │  │
│  │  │  - Hardware context (single-session)                           │ │  │
│  │  │  - ~100-200 MB/s throughput (SHA-256)                         │ │  │
│  │  └─────────────────────────────────────────────────────────────────┘ │  │
│  └───────────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Data Flow: Hash Operation

### Single Session (Simple Case)

```
Client Request
     │
     ▼
┌────────────────────────────────────────┐
│ 1. init_sha256()                       │
│    IPC → DigestServer                  │
└──────────┬─────────────────────────────┘
           │
           ▼
┌────────────────────────────────────────┐
│ 2. SessionManager::init_sha256()       │
│    - Allocates session slot            │
│    - Allocates provider session        │
│    - Sets active session               │
│    - Calls HaceController::init()      │
│    - Returns SessionDigest             │
└──────────┬─────────────────────────────┘
           │
           ▼
┌────────────────────────────────────────┐
│ 3. OwnedDigestContext created          │
│    - Wraps HaceController              │
│    - Initializes AspeedHashContext     │
│    - Copies IV to digest buffer        │
└──────────┬─────────────────────────────┘
           │
           ▼
┌────────────────────────────────────────┐
│ 4. HACE Hardware Initialization        │
│    - Set method register               │
│    - Set block size                    │
│    - Zero counters                     │
└──────────┬─────────────────────────────┘
           │
           ▼
     Return session_id
           │
     ┌─────┴─────────────────────────────┐
     │                                    │
     ▼                                    ▼
┌──────────────────┐            ┌──────────────────┐
│ 5. update(data)  │  (repeat)  │ 6. finalize()    │
│    IPC call      │◄───────────┤    IPC call      │
└────┬─────────────┘            └────┬─────────────┘
     │                               │
     ▼                               ▼
┌──────────────────┐            ┌──────────────────┐
│ SessionDigest    │            │ SessionManager   │
│ ::update()       │            │ ::finalize()     │
│ - Auto-activate  │            │ - Validate ID    │
│ - Copy to buffer │            │ - Finalize ctx   │
│ - If full, DMA   │            │ - Release session│
└────┬─────────────┘            └────┬─────────────┘
     │                               │
     ▼                               ▼
Return SessionDigest              Return Digest
```

### Multi-Session (Concurrent Case)

```
Multiple Clients (A, B, C)
     │    │    │
     ▼    ▼    ▼
┌────────────────────────────────────────┐
│ init_sha256()  init_sha384()  init_sha512()
│      │              │              │
│      ▼              ▼              ▼
│ SessionManager allocates 3 slots
│      │              │              │
│  session_id:0   session_id:1   session_id:2
└──────┬──────────────┬──────────────┬─────┘
       │              │              │
       │  Client A calls update()    │
       ▼                             │
┌─────────────────────┐              │
│ SessionDigest<256>  │              │
│ ::update()          │              │
│                     │              │
│ 1. Activate         │              │
│    session 0        │              │
│    ┌──────────┐     │              │
│    │ Provider │     │              │
│    │ .set_    │     │              │
│    │ active   │     │              │
│    │ (0)      │     │              │
│    └────┬─────┘     │              │
│         │           │              │
│    2. Switch if     │              │
│       needed        │              │
│    ┌──────────┐     │              │
│    │ save(1)  │     │              │
│    │ load(0)  │     │              │
│    └────┬─────┘     │              │
│         │           │              │
│    3. Update        │              │
│       context       │              │
└─────────┬───────────┘              │
          │                          │
          │  Client B calls update() │
          │                          ▼
          │                 ┌─────────────────────┐
          │                 │ SessionDigest<512>  │
          │                 │ ::update()          │
          │                 │                     │
          │                 │ 1. Activate         │
          │                 │    session 2        │
          │                 │    ┌──────────┐     │
          │                 │    │ Provider │     │
          │                 │    │ .set_    │     │
          │                 │    │ active   │     │
          │                 │    │ (2)      │     │
          │                 │    └────┬─────┘     │
          │                 │         │           │
          │                 │    2. Switch        │
          │                 │       contexts      │
          │                 │    ┌──────────┐     │
          │                 │    │ save(0)  │     │
          │                 │    │ load(2)  │     │
          │                 │    └────┬─────┘     │
          │                 │         │           │
          │                 │    3. Update        │
          │                 │       context       │
          │                 └─────────────────────┘
          │
    (Continues with more interleaved operations)
```

## Memory Layout

```
┌─────────────────────────────────────────────────────────────┐
│                      RAM (.ram_nc section)                  │
│                                                             │
│  ┌───────────────────────────────────────────────────────┐ │
│  │  SHARED_HASH_CTX (732 bytes)                          │ │
│  │  - Current active hardware context                    │ │
│  │  - Accessed via shared_hash_ctx()                     │ │
│  └───────────────────────────────────────────────────────┘ │
│                                                             │
│  For SingleContextProvider:                                │
│    - Only this context exists                              │
│                                                             │
│  For MultiContextProvider (N=4):                           │
│  ┌───────────────────────────────────────────────────────┐ │
│  │  contexts[0]: AspeedHashContext (732 bytes)           │ │
│  ├───────────────────────────────────────────────────────┤ │
│  │  contexts[1]: AspeedHashContext (732 bytes)           │ │
│  ├───────────────────────────────────────────────────────┤ │
│  │  contexts[2]: AspeedHashContext (732 bytes)           │ │
│  ├───────────────────────────────────────────────────────┤ │
│  │  contexts[3]: AspeedHashContext (732 bytes)           │ │
│  └───────────────────────────────────────────────────────┘ │
│                                                             │
│  Total: ~3 KB for 4-session multi-context provider         │
│                                                             │
│  SessionManager<8> State (~3.2 KB):                        │
│  ┌───────────────────────────────────────────────────────┐ │
│  │  controller: Option<HaceController> (16 bytes)        │ │
│  │  sessions: [SessionSlot; 8] (128 bytes)               │ │
│  │  next_id: u32 (4 bytes)                               │ │
│  │  + MultiContextProvider (~3 KB)                       │ │
│  └───────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

## Context Switching Sequence

```
┌──────────────────────────────────────────────────────────────────────┐
│                    Context Switch Timeline                           │
│                                                                      │
│  Time: 0 µs                                                         │
│  ┌──────────────────────────────────────────────────────┐           │
│  │  Session 0 active                                    │           │
│  │  Hardware context contains Session 0 state           │           │
│  └──────────────────────────────────────────────────────┘           │
│                                                                      │
│  Client calls SessionDigest<Session1>::update()                     │
│                                                                      │
│  Time: 0.1 µs                                                       │
│  ┌──────────────────────────────────────────────────────┐           │
│  │  1. Check: last_loaded (0) != active_id (1)         │           │
│  │     → Context switch needed                          │           │
│  └──────────────────────────────────────────────────────┘           │
│                                                                      │
│  Time: 0.2 µs                                                       │
│  ┌──────────────────────────────────────────────────────┐           │
│  │  2. Save Session 0 context                           │           │
│  │     - Copy 732 bytes from SHARED_HASH_CTX            │           │
│  │     - To contexts[0] storage                         │           │
│  │     - Time: ~10 µs @ 200 MHz                        │           │
│  └──────────────────────────────────────────────────────┘           │
│                                                                      │
│  Time: 10.2 µs                                                      │
│  ┌──────────────────────────────────────────────────────┐           │
│  │  3. Load Session 1 context                           │           │
│  │     - Copy 732 bytes from contexts[1]                │           │
│  │     - To SHARED_HASH_CTX                             │           │
│  │     - Time: ~10 µs @ 200 MHz                        │           │
│  └──────────────────────────────────────────────────────┘           │
│                                                                      │
│  Time: 20.2 µs                                                      │
│  ┌──────────────────────────────────────────────────────┐           │
│  │  4. Update last_loaded = Some(1)                    │           │
│  └──────────────────────────────────────────────────────┘           │
│                                                                      │
│  Time: 20.3 µs                                                      │
│  ┌──────────────────────────────────────────────────────┐           │
│  │  5. Perform actual update operation                  │           │
│  │     Hardware context now has Session 1 state         │           │
│  └──────────────────────────────────────────────────────┘           │
│                                                                      │
│  Total context switch overhead: ~20 µs                              │
│  (Only occurs when switching between different sessions)            │
└──────────────────────────────────────────────────────────────────────┘
```

## Trait Hierarchy

```
┌──────────────────────────────────────────────────────────────────┐
│              OpenProt HAL Traits (openprot-hal-blocking)         │
│                                                                  │
│  pub trait DigestAlgorithm {                                    │
│    const OUTPUT_BITS: usize;                                    │
│    type Digest;                                                 │
│  }                                                               │
│                                                                  │
│  pub mod owned {                                                │
│    pub trait DigestInit<A: DigestAlgorithm> {                  │
│      type Context: DigestOp;                                   │
│      type Output;                                               │
│                                                                  │
│      fn init(self, params: A) -> Result<Self::Context, ...>;  │
│    }                                                             │
│                                                                  │
│    pub trait DigestOp {                                         │
│      type Output;                                               │
│      type Controller;                                           │
│                                                                  │
│      fn update(self, data: &[u8]) -> Result<Self, ...>;       │
│      fn finalize(self) -> Result<(Output, Controller), ...>;   │
│      fn cancel(self) -> Self::Controller;                      │
│    }                                                             │
│  }                                                               │
└────────────────────┬─────────────────────────────────────────────┘
                     │ Implemented by
                     ▼
┌──────────────────────────────────────────────────────────────────┐
│                 aspeed-ddk Implementations                       │
│                                                                  │
│  impl<P> DigestInit<Sha2_256> for HaceController<P>             │
│  impl<P> DigestInit<Sha2_384> for HaceController<P>             │
│  impl<P> DigestInit<Sha2_512> for HaceController<P>             │
│                                                                  │
│  impl<P> DigestOp for OwnedDigestContext<Sha2_256, P>           │
│  impl<P> DigestOp for OwnedDigestContext<Sha2_384, P>           │
│  impl<P> DigestOp for OwnedDigestContext<Sha2_512, P>           │
└──────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────┐
│              Context Provider Trait                              │
│                                                                  │
│  pub trait HaceContextProvider {                                │
│    fn ctx_mut(&mut self) -> Result<&mut AspeedHashContext, E>; │
│  }                                                               │
└────────────────────┬─────────────────────────────────────────────┘
                     │ Implemented by
                     ▼
┌──────────────────────────────────────────────────────────────────┐
│  impl HaceContextProvider for SingleContextProvider              │
│    - Zero-cost: direct access to shared context                 │
│                                                                  │
│  impl HaceContextProvider for MultiContextProvider               │
│    - With context switching logic                               │
└──────────────────────────────────────────────────────────────────┘
```

## Component Responsibilities

| Layer | Component | Responsibilities |
|-------|-----------|-----------------|
| **IPC** | Client Tasks | Request digest operations, handle results |
| **Application** | DigestServerImpl | Handle IPC, validate inputs, manage session lifecycle |
| | SessionManager | High-level session API, automatic tracking |
| | SessionDigest | Auto-activating digest context |
| **Digest API** | OwnedDigestContext | Move-based digest operations, IPC-friendly |
| | OpContextImpl | Borrowed digest operations, non-IPC |
| **Multi-Context** | HaceController | Hardware controller abstraction |
| | MultiContextProvider | Session allocation, context switching |
| | SingleContextProvider | Zero-cost single session |
| **HAL** | AspeedHashContext | Hardware state storage |
| | shared_hash_ctx() | Global context accessor |
| **Hardware** | HACE Peripheral | DMA-based hash acceleration |

## Performance Budget

| Operation | Time | Memory |
|-----------|------|--------|
| IPC overhead | ~2-5 µs | Lease buffers (1 KB) |
| Session init | ~5-10 µs | Session slot (16 bytes) |
| Context switch | ~20-30 µs | Context copy (732 bytes) |
| Hash 1 KB (HW) | ~5-10 µs | DMA buffer (1 KB) |
| Hash 1 KB (SW) | ~100-200 µs | Stack usage |
| Session state | - | 16 bytes per session |
| Provider state | - | ~3 KB for N=4 |

## Design Principles

### 1. Layered Abstraction
Each layer has clear responsibilities and well-defined interfaces.

### 2. Zero-Cost Abstractions
- SingleContextProvider has no runtime overhead
- Generic programming enables compile-time optimization
- Move semantics prevent unnecessary copies

### 3. Type Safety
- Generic algorithm types prevent mismatches
- Session handles are type-safe (cannot confuse SHA-256 with SHA-384)
- Compile-time verification where possible

### 4. Resource Safety
- Automatic session cleanup on finalize/cancel
- RAII patterns prevent resource leaks
- Explicit error handling (no panics in production)

### 5. IPC-Friendly
- No lifetimes in stored types
- Move semantics allow transfer across IPC boundaries
- Predictable memory layout

## References

- [hace-server-hubris.md](../hace-server-hubris.md) - Original Hubris server design
- [multi-context-ipc-integration.md](multi-context-ipc-integration.md) - Integration analysis
- [multi-session-api-design.md](multi-session-api-design.md) - SessionManager API spec
- [hace-multi-context-design.md](hace-multi-context-design.md) - Multi-context provider design

---

**Document Version**: 1.0
**Last Updated**: 2025-09-30
**Author**: Claude Code
