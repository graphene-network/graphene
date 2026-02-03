# Payment Ticket Module

Off-chain payment ticket format for zero-latency job payments with worker-side local state management.

## Overview

This module implements the payment ticket system (Issues #27, #28) that enables workers to verify payment authorization in <1ms without on-chain lookups.

```
┌─────────────────────────────────────────────────────────────┐
│                    TICKET VERIFICATION                       │
├─────────────────────────────────────────────────────────────┤
│  1. Signature Check (<0.1ms) - Ed25519 verify               │
│  2. Local State Check (<0.1ms) - nonce, amount, balance     │
│  3. Timestamp Check (<0.1ms) - staleness window             │
│  Total: <1ms verification (actual: ~226µs)                  │
└─────────────────────────────────────────────────────────────┘
```

## Architecture

### Ticket Format

The ticket separates signed payload from unsigned envelope:

| Component | Size | Description |
|-----------|------|-------------|
| `channel_id` | 32 bytes | Payment channel PDA address |
| `amount_micros` | 8 bytes | Cumulative amount authorized (LE u64) |
| `nonce` | 8 bytes | Monotonically increasing (LE u64) |
| `signature` | 64 bytes | Ed25519 over 48-byte payload |
| `timestamp` | 8 bytes | Unix epoch (unsigned, staleness only) |

The 48-byte signed payload (`channel_id || amount_micros || nonce`) is compatible with on-chain Ed25519 verification for settlement.

### Local State Management

Workers maintain `ChannelLocalState` per payment channel:

```rust
pub struct ChannelLocalState {
    pub channel_id: [u8; 32],        // PDA address
    pub user: [u8; 32],              // User's Ed25519 pubkey
    pub worker: [u8; 32],            // Our pubkey
    pub on_chain_balance: u64,       // Last known from chain
    pub accepted_amount: u64,        // Sum of accepted tickets
    pub last_settled_amount: u64,    // Confirmed on-chain
    pub last_nonce: u64,             // Highest accepted nonce
    pub highest_ticket: Option<PaymentTicket>,  // For settlement
    pub on_chain_state: OnChainChannelState,    // Open/Closing
    // ...
}
```

### Background Services

`ChannelSyncService` provides three background safeguards:

1. **Periodic Sync** - Fetch on-chain state every 10 min (configurable)
2. **Threshold Monitor** - Check unsettled amounts every 60s, trigger settlement
3. **WebSocket Subscriptions** - Real-time channel updates from Solana

## Files

| File | Purpose |
|------|---------|
| `types.rs` | `PaymentTicket`, `TicketPayload`, `ChannelState` |
| `signer.rs` | `TicketSigner` trait, `DefaultTicketSigner` |
| `validator.rs` | `TicketValidator` trait, `DefaultTicketValidator` |
| `channel_state.rs` | `ChannelLocalState`, `ChannelEvent`, `ChannelStateManager` trait |
| `channel_manager.rs` | `DefaultChannelStateManager` implementation |
| `solana_client.rs` | `SolanaChannelClient` trait for on-chain queries |
| `channel_sync.rs` | `ChannelSyncService` background tasks |
| `mock.rs` | Mock implementations for testing |

## Usage

### Signing a Ticket (User/Client)

```rust
use monad_node::ticket::{DefaultTicketSigner, TicketSigner};

let signer = DefaultTicketSigner::from_bytes(&user_secret_key);
let ticket = signer.sign_ticket(channel_id, amount_micros, nonce).await?;
```

### Validating a Ticket (Worker)

```rust
use monad_node::ticket::{DefaultTicketValidator, TicketValidator, ChannelState};

let validator = DefaultTicketValidator::new();
let channel_state = ChannelState {
    last_nonce: 0,
    last_amount: 0,
    channel_balance: 10_000_000,
};
validator.validate(&ticket, &user_pubkey, &channel_state).await?;
```

### Full Channel State Management (Worker)

```rust
use monad_node::ticket::{
    DefaultChannelStateManager, ChannelConfig, ChannelLocalState,
    ChannelSyncService, MockSolanaChannelClient,
};
use std::sync::Arc;

// Create manager with default validator
let config = ChannelConfig::default();
let manager = Arc::new(DefaultChannelStateManager::with_default_validator(config.clone()));

// Register a channel
let channel = ChannelLocalState { /* ... */ };
manager.upsert_channel(channel).await?;

// Accept tickets (validates + updates state)
let result = manager.accept_ticket(&channel_id, &ticket).await?;
match result {
    TicketAcceptResult::Accepted { new_amount, unsettled, needs_settlement } => {
        if needs_settlement {
            // Trigger settlement with highest_ticket
        }
    }
    TicketAcceptResult::Rejected(err) => {
        // Handle validation failure
    }
}

// Start background sync service
let solana_client = Arc::new(MockSolanaChannelClient::new());
let sync_service = ChannelSyncService::new(
    manager,
    solana_client,
    config,
    |channel_id| { /* settlement callback */ },
);
sync_service.start().await?;
```

## Validation Rules

Tickets are validated in this order:

1. **Signature** - Ed25519 signature must be valid for payer's pubkey
2. **Nonce** - Must be strictly greater than last seen nonce (replay protection)
3. **Amount** - Must be >= last amount (cumulative) and <= channel balance
4. **Timestamp** - Must be within ±5 minutes of current time (staleness)

## Configuration

```rust
pub struct ChannelConfig {
    pub max_unsettled_threshold: u64,  // Default: 10_000_000 (10 USDC)
    pub sync_interval_secs: u64,       // Default: 600 (10 min)
    pub max_staleness_secs: u64,       // Default: 1800 (30 min)
}
```

## On-Chain Channel Format

The `SolanaChannelClient` parses on-chain `PaymentChannel` accounts (138 bytes):

```
8 bytes  - Anchor discriminator
32 bytes - user pubkey
32 bytes - worker pubkey
32 bytes - mint pubkey
8 bytes  - balance (u64 LE)
8 bytes  - spent (u64 LE)
8 bytes  - last_nonce (u64 LE)
8 bytes  - timeout (i64 LE)
1 byte   - state (0=Open, 1=Closing)
1 byte   - bump
```

PDA derivation: `[b"channel", user.key(), worker.key()]`

## Testing

```bash
# Run all ticket tests
cargo test -p monad-node ticket::

# Run benchmarks
cargo test -p monad-node bench_ticket --nocapture
cargo test -p monad-node bench_accept --nocapture
```

## Performance

Benchmarks show ~226µs average validation time, well under the 1ms target:

- Ed25519 signature verification: ~200µs
- State lookup + update: ~2µs
- Nonce/amount/timestamp checks: <1µs

## Related Issues

- [#27](https://github.com/marcus-sa/monad/issues/27) - Payment ticket format
- [#28](https://github.com/marcus-sa/monad/issues/28) - Worker-side ticket verification
- [#30](https://github.com/marcus-sa/monad/issues/30) - Batch settlement (blocked by this)
