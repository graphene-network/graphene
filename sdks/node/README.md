# @graphene/sdk

TypeScript SDK for Graphene Network - decentralized unikernel compute.

## Installation

```bash
npm install @graphene/sdk
```

## Quick Start

```typescript
import { Client } from '@graphene/sdk';

const client = await Client.create({
  secretKey: mySecretKey,  // Your Ed25519 secret key (32 bytes)
  channelPda: channelPda,  // Solana payment channel PDA (32 bytes)
  workerNodeId: nodeId,    // Worker's node ID (hex-encoded Ed25519 pubkey)
});

const result = await client.run({
  code: 'print(2 + 2)',
  runtime: 'python:3.12',
});

console.log(new TextDecoder().decode(result.output)); // "4\n"

await client.close();
```

## Features

- **End-to-end encryption**: All code and data is encrypted using XChaCha20-Poly1305
- **Forward secrecy**: Per-job ephemeral keys ensure past jobs can't be decrypted
- **Payment channels**: Off-chain payment tickets for efficient micropayments
- **Native performance**: Crypto, networking, and protocol handling in Rust via NAPI

## Architecture

This SDK is a thin TypeScript wrapper over native Rust bindings. All heavy lifting happens in Rust:

- Channel key derivation (Ed25519 → X25519 → HKDF)
- Job encryption/decryption (XChaCha20-Poly1305)
- Payment ticket creation (Ed25519 signatures)
- Blob upload/download (BLAKE3 content-addressing)
- Protocol serialization (bincode)
- Network transport (QUIC via Iroh)

## API Reference

### `Client.create(config)`

Create a new Graphene client.

**Parameters:**
- `config.secretKey` - Your Ed25519 secret key (32 bytes)
- `config.channelPda` - Solana payment channel PDA (32 bytes)
- `config.workerNodeId` - Worker's node ID (64 hex chars = Ed25519 pubkey)
- `config.storagePath?` - Storage path for persistent data (default: '.graphene-sdk')
- `config.useRelay?` - Enable relay servers for NAT traversal (default: true)
- `config.bindPort?` - Local port to bind (default: random)

**Returns:** `Promise<Client>`

### `client.run(options)`

Run a job on a Graphene worker.

**Parameters:**
- `options.code` - Code to execute (string)
- `options.input?` - Optional input data (Uint8Array)
- `options.kernel?` - Runtime image (default: "python:3.12")
- `options.vcpu?` - vCPU count (default: 1)
- `options.memoryMb?` - Memory in MB (default: 256)
- `options.timeoutMs?` - Timeout in ms (default: 30000)
- `options.env?` - Environment variables (Record<string, string>)
- `options.egressAllowlist?` - Allowed egress endpoints
- `options.deliveryMode?` - 'sync' (default) or 'async'

**Returns:** `Promise<RunResult>`
- `exitCode` - Exit code (0 = success)
- `output` - Decrypted output (Uint8Array)
- `durationMs` - Execution time in milliseconds
- `metrics` - Resource usage metrics

### `client.nodeId()`

Get this client's node ID (public key) as a hex string.

**Returns:** `Promise<string>`

### `client.currentNonce`

Get the current nonce value. Useful for tracking payment channel state.

### `client.totalAuthorized`

Get the cumulative amount authorized across all payment tickets.

### `client.close()`

Close the client and release resources.

## Error Handling

The SDK provides specific error classes for different failure modes:

```typescript
import {
  JobRejectedError,
  JobFailedError,
  JobTimeoutError,
  TransportError,
  CryptoError,
  ConfigError,
} from '@graphene/sdk';

try {
  await client.run({ code: 'bad code' });
} catch (error) {
  if (error instanceof JobRejectedError) {
    console.log('Rejected:', error.reason);
  } else if (error instanceof JobFailedError) {
    console.log('Exit code:', error.exitCode);
    console.log('Output:', new TextDecoder().decode(error.output));
  } else if (error instanceof JobTimeoutError) {
    console.log('Timeout after:', error.timeoutMs, 'ms');
  }
}
```

## Supported Kernels

- `python:3.10` - Python 3.10
- `python:3.12` - Python 3.12 (default)
- `node:20` - Node.js 20
- `node:21` - Node.js 21
- `bun:1.1` - Bun 1.1

## Advanced: Native Functions

For advanced use cases, you can access the native crypto functions directly:

```typescript
import {
  deriveChannelKeys,
  encryptJobBlob,
  decryptJobBlob,
  createPaymentTicket,
  blake3Hash,
} from '@graphene/sdk';
```

## Security

- All communication is encrypted end-to-end
- Jobs run in isolated unikernels with no shell access
- Network egress is restricted to explicit allowlists
- Payment tickets use Ed25519 signatures

## License

AGPL-3.0-only
