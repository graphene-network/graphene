# Payment Ticket Module

Off-chain payment ticket format for zero-latency job payments with Ed25519 signatures and worker-side validation.

## Overview

This module implements the payment ticket specification from [Issue #27](https://github.com/marcus-sa/monad/issues/27), providing cryptographically signed tickets that authorize workers to claim payment from user payment channels.

### Key Design Decision

**Signature format**: The 48-byte signed message (`channel_id || amount_micros || nonce`) is compatible with on-chain Ed25519 verification. The timestamp is **unsigned envelope metadata** for operational staleness checks only.

This allows the same signature to work for both:
- Off-chain validation (fast, worker-side)
- On-chain settlement (when workers claim payment)

## Usage

### Signing Tickets (User/Client Side)

```rust
use monad_node::ticket::{DefaultTicketSigner, TicketSigner};

let signer = DefaultTicketSigner::from_bytes(&user_secret_key);

// Sign a ticket authorizing 1,000,000 microtokens
let ticket = signer.sign_ticket(
    channel_id,      // [u8; 32] - Payment channel address
    1_000_000,       // amount_micros - Cumulative amount
    5,               // nonce - Must be > last nonce
).await?;
```

### Validating Tickets (Worker Side)

```rust
use monad_node::ticket::{
    DefaultTicketValidator, TicketValidator, ChannelState,
};

let validator = DefaultTicketValidator::new();

// Track channel state per-user
let channel_state = ChannelState {
    last_nonce: 4,
    last_amount: 500_000,
    channel_balance: 10_000_000,
};

// Validate returns Ok(()) or a specific TicketError
validator.validate(&ticket, &user_pubkey, &channel_state).await?;
```

### Testing with Mocks

```rust
use monad_node::ticket::{MockTicketValidator, MockValidatorBehavior};

// Always accept
let validator = MockTicketValidator::always_valid();

// Always reject with specific error
let validator = MockTicketValidator::always_invalid_signature();

// Accept first N tickets, then reject
let validator = MockTicketValidator::accept_first(5);

// Track call count
assert_eq!(validator.call_count(), 0);
validator.validate(...).await?;
assert_eq!(validator.call_count(), 1);
```

## Validation Rules

Tickets are validated in this order:

| Rule | Error | Description |
|------|-------|-------------|
| Signature | `InvalidSignature` | Ed25519 signature must be valid |
| Nonce | `ReplayedNonce` | Must be > last seen nonce |
| Amount | `NonCumulativeAmount` | Must be >= last amount (cumulative) |
| Amount | `InsufficientBalance` | Must be <= channel balance |
| Timestamp | `FutureTimestamp` | Must be <= now + 60 seconds |
| Timestamp | `StaleTimestamp` | Must be >= now - 300 seconds |

## Wire Format

### Signed Payload (48 bytes)

```
┌──────────────────────────────────────────────────┐
│ channel_id (32 bytes)                            │
├──────────────────────────────────────────────────┤
│ amount_micros (8 bytes, little-endian u64)       │
├──────────────────────────────────────────────────┤
│ nonce (8 bytes, little-endian u64)               │
└──────────────────────────────────────────────────┘
```

This format matches the on-chain verification in `programs/graphene/src/utils/ed25519.rs`.

### Full Ticket (JSON)

```json
{
  "channel_id": [/* 32 bytes */],
  "amount_micros": 1000000,
  "nonce": 5,
  "timestamp": 1700000000,
  "signature": [/* 64 bytes */]
}
```

## Performance

Benchmarks show excellent performance, well under the 1ms requirement:

| Operation | Time |
|-----------|------|
| Ticket signing | ~10 µs |
| Ticket validation | ~28 µs |
| Ed25519 verify only | ~28 µs |
| Payload serialization | ~7 ns |

Run benchmarks with:
```bash
cargo bench --bench ticket_validation -p monad_node
```

## Module Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Module exports, `TicketError` enum |
| `types.rs` | `PaymentTicket`, `TicketPayload`, `ChannelState`, `Signature64` |
| `signer.rs` | `TicketSigner` trait + `DefaultTicketSigner` |
| `validator.rs` | `TicketValidator` trait + `DefaultTicketValidator` |
| `mock.rs` | `MockTicketValidator` for testing |
