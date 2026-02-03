# Crypto Module - Agent Context

## Purpose

End-to-end encryption for job inputs/outputs, enabling "soft confidential computing" without TEE hardware.

## Key Concepts

### Two-Layer Key Derivation

1. **Channel Master Key** (long-lived): ECDH between user/worker X25519 keys + channel PDA salt
2. **Per-Job Key** (ephemeral): Random X25519 keypair per job provides forward secrecy

### Why This Matters

- **Payment binding**: Only parties with valid payment channel can decrypt
- **Forward secrecy**: Compromised channel keys don't expose past jobs
- **No new infrastructure**: Reuses existing Ed25519 identity keys (converted to X25519)

## Files

| File | Purpose |
|------|---------|
| `mod.rs` | `CryptoProvider` trait, `DefaultCryptoProvider`, `EncryptionDirection` |
| `channel_keys.rs` | Ed25519→X25519 conversion, `ChannelKeys` derivation |
| `job_crypto.rs` | `encrypt_blob()`, `decrypt_blob()`, `EncryptedBlob` format |
| `mock.rs` | `MockCryptoProvider` with configurable failure modes |

## Usage Pattern

```rust
use monad_node::crypto::{CryptoProvider, DefaultCryptoProvider, EncryptionDirection};

let crypto = DefaultCryptoProvider;

// 1. Derive channel keys (once per payment channel relationship)
let channel_keys = crypto.derive_channel_keys(
    &my_ed25519_secret,      // From Iroh identity
    &peer_ed25519_public,    // Peer's Iroh node ID
    &channel_pda,            // Solana payment channel address
)?;

// 2. Encrypt job input (user side)
let encrypted = crypto.encrypt_job_blob(
    plaintext,
    &channel_keys,
    &job_id,
    EncryptionDirection::Input,
)?;

// 3. Decrypt job input (worker side)
let decrypted = crypto.decrypt_job_blob(
    &encrypted,
    &channel_keys,  // Worker derives same keys from their secret
    &job_id,
    EncryptionDirection::Input,
)?;
```

## Encrypted Blob Format

```
[version: 1 byte]
[ephemeral_pubkey: 32 bytes]
[nonce: 24 bytes]
[ciphertext + tag: N + 16 bytes]
```

Self-contained: everything needed to decrypt is in the blob (except the keys).

## Security Properties

| Property | How Achieved |
|----------|--------------|
| Confidentiality | XChaCha20-Poly1305 |
| Authentication | Poly1305 tag |
| Payment binding | Channel PDA in HKDF salt |
| Forward secrecy | Ephemeral X25519 per job |
| Replay protection | Job ID in key derivation |
| Direction separation | Different HKDF info strings |

## What Gets Encrypted

- ✅ Input blob (user data)
- ✅ Code blob (user logic)
- ✅ Result blob (computation output)
- ✅ stdout/stderr
- ❌ Job manifest (worker needs for resource allocation)
- ❌ Exit code (state machine needs)

## Testing

```bash
cargo test -p monad_node crypto
```

Mock behaviors for testing:
- `MockCryptoBehavior::Normal` - Works correctly
- `MockCryptoBehavior::AlwaysFail(msg)` - All operations fail
- `MockCryptoBehavior::FailAfter(n)` - Fail after N operations
- `MockCryptoBehavior::CorruptCiphertext` - Tamper with output

## Dependencies

```toml
x25519-dalek = "2.0"
chacha20poly1305 = "0.10"
hkdf = "0.12"
ed25519-dalek = "2.0"
curve25519-dalek = "4.1"
zeroize = "1.7"
```
