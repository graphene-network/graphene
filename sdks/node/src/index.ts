/**
 * Graphene SDK - High-level TypeScript client for Graphene Network.
 *
 * This SDK is a thin wrapper around native Rust bindings.
 * All cryptography, networking, and protocol handling is done in Rust.
 *
 * @packageDocumentation
 * @module @graphene/sdk
 *
 * @example
 * ```typescript
 * import { Client } from '@graphene/sdk';
 *
 * const client = await Client.create({
 *   secretKey: mySecretKey,  // Your Ed25519 secret key (32 bytes)
 *   channelPda: channelPda,  // Payment channel PDA (32 bytes)
 *   workerNodeId: nodeId,    // Worker's node ID (hex-encoded Ed25519 pubkey)
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
  GrapheneError,
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
  PaymentTicket,
  ChannelState,
  JobRequest,
  JobResponse,
  JobManifest,
  JobAssets,
  JobResult,
  JobMetrics,
  EgressRule,
  WireMessage,
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
export { GrapheneClient } from '@graphene/sdk-native';

// Re-export native functions for advanced usage
export {
  deriveChannelKeys,
  encryptJobBlob,
  decryptJobBlob,
  createPaymentTicket,
  verifyTicketSignature,
  validateTicket,
  serializeJobRequest,
  deserializeJobResponse,
  encodeWireMessage,
  decodeWireMessage,
  blake3Hash,
} from '@graphene/sdk-native';
