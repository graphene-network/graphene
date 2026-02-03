/**
 * Graphene SDK - High-level TypeScript client for Graphene Network.
 *
 * @packageDocumentation
 * @module @graphene/sdk
 *
 * @example
 * ```typescript
 * import { Client } from '@graphene/sdk';
 *
 * const client = new Client({
 *   secretKey: mySecretKey,      // Your Ed25519 secret key (32 bytes)
 *   workerPubkey: workerPubkey,  // Worker's public key (32 bytes)
 *   channelPda: channelPda,      // Payment channel PDA (32 bytes)
 * });
 *
 * const result = await client.run({
 *   code: 'print(2 + 2)',
 *   kernel: 'python:3.12',
 * });
 *
 * console.log(new TextDecoder().decode(result.output)); // "4\n"
 * ```
 */

// Main client
export { Client } from './client.js';

// Transport implementations
export { MockTransport, IrohTransport, HttpGatewayTransport } from './transport.js';

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
// Native types (interfaces)
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
  Transport,
  JobProgress,
  EgressRuleConfig,
} from './types.js';

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
} from '@graphene/sdk-native';
