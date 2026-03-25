/**
 * OpenCapsule SDK - High-level TypeScript client for OpenCapsule Network.
 *
 * This SDK is a thin wrapper around native Rust bindings.
 * All cryptography, networking, and protocol handling is done in Rust.
 *
 * @packageDocumentation
 * @module @opencapsule/sdk
 *
 * @example
 * ```typescript
 * import { Client } from '@opencapsule/sdk';
 *
 * const client = await Client.create({
 *   secretKey: mySecretKey,    // Your Ed25519 secret key (32 bytes)
 *   channelId: channelId,      // Shared channel identifier (32 bytes)
 *   workerPubkey: pubkey,      // Worker's Ed25519 public key (hex)
 *   workerUrl: 'http://worker:3000',
 * });
 *
 * const result = await client.run({
 *   code: 'print(2 + 2)',
 *   runtime: 'python:3.12',
 * });
 *
 * console.log(new TextDecoder().decode(result.output)); // "4\n"
 *
 * await client.close();
 * ```
 */

// Main client
export { Client } from './client.js';

// Error classes
export {
  OpenCapsuleError,
  JobRejectedError,
  JobFailedError,
  JobTimeoutError,
  TransportError,
  CryptoError,
  PaymentError,
  ConfigError,
} from './errors.js';

// Types (re-exports from native + additional types)
export type {
  ChannelKeys,
  EncryptedBlob,
  NativeClientConfig,
  NativeJobOptions,
  NativeJobResult,
} from './types.js';

// Native enums (need value export for runtime)
export {
  EncryptionDirection,
  JobStatus,
  RejectReason,
} from './types.js';

// SDK types
export type {
  ClientConfig,
  RunOptions,
  RunResult,
  EgressRuleConfig,
} from './types.js';

// Re-export native client for advanced usage
export { OpenCapsuleClient } from '@opencapsule/sdk-native';

// Re-export native functions for advanced usage
export {
  deriveChannelKeys,
  encryptJobBlob,
  decryptJobBlob,
  blake3Hash,
} from '@opencapsule/sdk-native';
