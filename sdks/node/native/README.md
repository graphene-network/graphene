# @opencapsule/sdk-native

Native bindings for OpenCapsule SDK - cryptographic primitives, payment tickets, and protocol serialization.

This package provides the low-level native bindings used by `@opencapsule/sdk`. Most users should use `@opencapsule/sdk` directly.

## Installation

```bash
npm install @opencapsule/sdk-native
```

## Supported Platforms

Pre-built binaries are available for:

| Platform | Architecture | libc |
|----------|-------------|------|
| Linux    | x86_64      | glibc |
| Linux    | ARM64       | glibc |
| Linux    | x86_64      | musl |
| Linux    | ARM64       | musl |
| macOS    | x86_64      | - |
| macOS    | ARM64 (Apple Silicon) | - |

## API Overview

### Channel Key Derivation

```typescript
import { deriveChannelKeys } from '@opencapsule/sdk-native';

const channelKeys = deriveChannelKeys(
  localSecret,   // Ed25519 secret key (32 bytes)
  peerPubkey,    // Ed25519 public key (32 bytes)
  channelPda     // Solana PDA (32 bytes)
);

const masterKey = channelKeys.masterKey();      // Shared 32-byte key
const peerPubKey = channelKeys.peerPublicKey(); // X25519 public key
```

### Job Encryption

```typescript
import {
  encryptJobBlob,
  decryptJobBlob,
  EncryptionDirection,
} from '@opencapsule/sdk-native';

// Encrypt data for a job
const encrypted = encryptJobBlob(
  plaintext,                    // Buffer
  channelKeys,                  // From deriveChannelKeys
  jobId,                        // Unique job ID string
  EncryptionDirection.Input     // Input or Output
);

// Access encrypted blob fields
encrypted.version;         // Format version (1)
encrypted.ephemeralPubkey; // 32-byte ephemeral key
encrypted.nonce;           // 24-byte nonce
encrypted.ciphertext;      // Encrypted data + auth tag

// Serialize/deserialize
const bytes = encrypted.toBytes();
const restored = EncryptedBlob.fromBytes(bytes);

// Decrypt
const decrypted = decryptJobBlob(
  encrypted,
  channelKeys,
  jobId,
  EncryptionDirection.Input
);
```

### Payment Tickets

```typescript
import {
  createPaymentTicket,
  verifyTicketSignature,
  validateTicket,
} from '@opencapsule/sdk-native';

// Create a payment ticket
const ticket = createPaymentTicket(
  channelId,     // 32-byte channel address
  amountMicros,  // BigInt amount in microtokens
  nonce,         // BigInt sequence number
  signerSecret   // Ed25519 secret key
);

// Access ticket fields
ticket.channelId;     // 32-byte Buffer
ticket.amountMicros;  // BigInt
ticket.nonce;         // BigInt
ticket.timestamp;     // Unix epoch seconds
ticket.signature();   // 64-byte Ed25519 signature

// Verify signature
const isValid = verifyTicketSignature(ticket, payerPubkey);

// Full validation against channel state
await validateTicket(ticket, payerPubkey, {
  lastNonce: 0n,
  lastAmount: 0n,
  channelBalance: 10000000n,
});
```

### Protocol Serialization

```typescript
import {
  serializeJobRequest,
  deserializeJobResponse,
  encodeWireMessage,
  decodeWireMessage,
} from '@opencapsule/sdk-native';

// Serialize a job request to wire format
const request = {
  jobId: 'uuid-string',
  manifest: {
    vcpu: 1,
    memoryMb: 256,
    timeoutMs: 30000n,
    runtime: 'python:3.12',
    egressAllowlist: [],
    env: {},
  },
  ticket: ticketBytes,
  assets: {
    codeHash: Buffer.alloc(32),
    inputHash: Buffer.alloc(32),
  },
  ephemeralPubkey: Buffer.alloc(32),
  channelPda: Buffer.alloc(32),
  deliveryMode: 'sync',
};

const wireBytes = serializeJobRequest(request);

// Deserialize a job response
const response = deserializeJobResponse(wireBytes);
console.log(response.status);  // 'Accepted', 'Running', 'Succeeded', etc.
console.log(response.result);  // Job result if completed

// Low-level wire message encoding
const msg = encodeWireMessage(1, payload);  // Type 1 = JobRequest
const decoded = decodeWireMessage(msg);
console.log(decoded.msgType, decoded.payload);
```

## Cryptographic Details

### Key Derivation
1. Ed25519 keys are converted to X25519
2. X25519 ECDH produces a shared secret
3. HKDF with channel PDA as salt derives the master key

### Encryption
- Algorithm: XChaCha20-Poly1305
- Nonce: 24 bytes, randomly generated
- Per-job ephemeral keys for forward secrecy
- Job ID incorporated into key derivation

### Signatures
- Algorithm: Ed25519
- Ticket payload: channel_id (32) || amount (8) || nonce (8)

## Wire Protocol

Messages use a simple length-prefixed format:

```
[4 bytes: length (BE)] [1 byte: type] [N bytes: bincode payload]
```

Message types:
- 1: JobRequest
- 2: JobAccepted
- 3: JobProgress
- 4: JobResult
- 5: JobRejected

## Building from Source

Requires Rust 1.70+ and Node.js 18+.

```bash
cd crates/sdk
npm install
npm run build
```

## Testing

```bash
npm test
```

## License

AGPL-3.0-only
