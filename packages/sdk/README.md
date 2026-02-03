# @graphene/sdk

TypeScript SDK for Graphene Network - decentralized unikernel compute.

## Installation

```bash
npm install @graphene/sdk
```

## Quick Start

```typescript
import { Client } from '@graphene/sdk';

const client = new Client({
  secretKey: mySecretKey,      // Your Ed25519 secret key (32 bytes)
  workerPubkey: workerPubkey,  // Worker's Ed25519 public key (32 bytes)
  channelPda: channelPda,      // Solana payment channel PDA (32 bytes)
});

const result = await client.run({
  code: 'print(2 + 2)',
  kernel: 'python:3.12',
});

console.log(new TextDecoder().decode(result.output)); // "4\n"

await client.close();
```

## Features

- **End-to-end encryption**: All code and data is encrypted using XChaCha20-Poly1305
- **Forward secrecy**: Per-job ephemeral keys ensure past jobs can't be decrypted
- **Payment channels**: Off-chain payment tickets for efficient micropayments
- **Progress tracking**: Real-time progress callbacks during job execution
- **Multiple transports**: Support for QUIC (Iroh), HTTP gateway, and mock transport

## API Reference

### `new Client(config)`

Create a new Graphene client.

**Parameters:**
- `config.secretKey` - Your Ed25519 secret key (32 bytes)
- `config.workerPubkey` - Worker's Ed25519 public key (32 bytes)
- `config.channelPda` - Solana payment channel PDA (32 bytes)
- `config.transport?` - Optional custom transport (default: MockTransport)

### `client.run(options)`

Run a job on a Graphene worker.

**Parameters:**
- `options.code` - Code to execute (string)
- `options.input?` - Optional input data (Uint8Array)
- `options.kernel?` - Kernel image (default: "python:3.12")
- `options.vcpu?` - vCPU count (default: 1)
- `options.memoryMb?` - Memory in MB (default: 256)
- `options.timeoutMs?` - Timeout in ms (default: 30000)
- `options.env?` - Environment variables (Record<string, string>)
- `options.egressAllowlist?` - Allowed egress endpoints
- `options.onProgress?` - Progress callback

**Returns:** `Promise<RunResult>`
- `exitCode` - Exit code (0 = success)
- `output` - Decrypted output (Uint8Array)
- `durationMs` - Execution time in milliseconds
- `metrics` - Resource usage metrics

### `client.encrypt(data, jobId, direction)`

Encrypt data for a specific job.

**Parameters:**
- `data` - Data to encrypt (Uint8Array)
- `jobId` - Job ID for key derivation (string)
- `direction` - 'input' (user->worker) or 'output' (worker->user)

**Returns:** `Uint8Array` - Serialized encrypted blob

### `client.decrypt(data, jobId, direction)`

Decrypt data from a specific job.

**Parameters:**
- `data` - Serialized encrypted blob (Uint8Array)
- `jobId` - Job ID used for encryption (string)
- `direction` - Direction used during encryption

**Returns:** `Uint8Array` - Decrypted data

### `client.currentNonce`

Get the current nonce value. Useful for tracking payment channel state.

### `client.totalAuthorized`

Get the cumulative amount authorized across all payment tickets.

### `client.close()`

Close the client and release resources.

## Transport Options

### MockTransport (default)

For testing without network access:

```typescript
import { MockTransport } from '@graphene/sdk';

const client = new Client({
  // ... keys
  transport: new MockTransport({ delay: 100 }),
});
```

### HttpGatewayTransport

For development through an HTTP gateway:

```typescript
import { HttpGatewayTransport } from '@graphene/sdk';

const client = new Client({
  // ... keys
  transport: new HttpGatewayTransport('https://gateway.graphene.network'),
});
```

### IrohTransport

For direct QUIC connectivity to workers (when available):

```typescript
import { IrohTransport } from '@graphene/sdk';

const client = new Client({
  // ... keys
  transport: new IrohTransport(workerNodeId),
});
```

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

## Security

- All communication is encrypted end-to-end
- Jobs run in isolated unikernels with no shell access
- Network egress is restricted to explicit allowlists
- Payment tickets use Ed25519 signatures

## License

AGPL-3.0-only
