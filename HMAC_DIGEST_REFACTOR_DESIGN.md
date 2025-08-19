# HMAC Implementation Design Document
## Building HMAC on Digest Traits for Aspeed HACE Controller

**Version:** 1.0  
**Date:** August 18, 2025  
**Author:** GitHub Copilot  

---

## Executive Summary

This document outlines the design for refactoring the HMAC implementation in the Aspeed HACE controller to be built on top of the existing digest trait implementations. The current approach maintains separate implementations for digest and HMAC operations, leading to code duplication and maintenance overhead. The proposed design leverages the standard HMAC construction using proven digest implementations.

## Current State Analysis

### Existing Architecture

```
┌─────────────────┐    ┌─────────────────┐
│   Digest Impl   │    │   HMAC Impl     │
│                 │    │                 │
│ HashContext     │    │ HmacContext     │
│ DigestOp        │    │ MacOp           │
│ Infallible      │    │ Custom Error    │
└─────────────────┘    └─────────────────┘
         │                       │
         └───────────────────────┘
                     │
         ┌─────────────────────────┐
         │   HaceController        │
         │                         │
         │ Hardware-specific       │
         │ HMAC methods:           │
         │ - init_hmac_sha256()    │
         │ - init_hmac_sha384()    │
         │ - init_hmac_sha512()    │
         └─────────────────────────┘
```

### Problems Identified

1. **Code Duplication**: Separate hardware interaction logic for digest and HMAC
2. **Inconsistent Error Handling**: `Infallible` for digest vs custom `Error` for HMAC  
3. **Hardware-Specific Methods**: Direct HMAC hardware methods bypass digest abstraction
4. **Maintenance Overhead**: Changes to hash algorithms require updates in two places
5. **Testing Complexity**: Separate test harnesses and validation logic

## Design Goals

### Primary Objectives
- **Code Reuse**: Eliminate duplication between digest and HMAC implementations
- **Standard Compliance**: Implement HMAC using the standard RFC 2104 construction
- **Hardware Optimization**: Maintain performance benefits of hardware acceleration
- **API Compatibility**: Preserve existing OpenPRoT trait interfaces
- **Maintainability**: Single source of truth for hash algorithm implementations

### Non-Goals
- Changing OpenPRoT MAC or Digest trait interfaces
- Breaking compatibility with existing test code
- Removing hardware-specific optimizations entirely

## Proposed Architecture

### High-Level Design

```
┌─────────────────────────────────────────────────────┐
│                HMAC Implementation                   │
│                                                     │
│  HmacContext<H> where H: DigestAlgorithm           │
│  ┌─────────────┐    ┌─────────────┐                │
│  │ Inner Hash  │    │ Outer Hash  │                │
│  │ H(K⊕ipad||m)│    │ H(K⊕opad||  │                │
│  │             │    │   inner)    │                │
│  └─────────────┘    └─────────────┘                │
└─────────────────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────┐
│              Digest Implementation                   │
│                                                     │
│  HashContext<H> where H: DigestAlgorithm           │
│  - DigestOp: update(), finalize()                  │
│  - Hardware abstraction                            │
│  - Error handling (Infallible)                     │
└─────────────────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────┐
│                HaceController                        │
│                                                     │
│  - Low-level hardware operations                   │
│  - Context management                               │
│  - Register manipulation                            │
└─────────────────────────────────────────────────────┘
```
