/**
 * Type definitions for the Graphene SDK.
 *
 * Re-exports types from the native bindings and adds additional
 * TypeScript-specific types for the high-level client.
 *
 * @module types
 */

// Re-export types from native bindings
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
  // Native client types
  ClientConfig as NativeClientConfig,
  JobOptions as NativeJobOptions,
  NativeJobResult,
} from '@graphene/sdk-native';

// Re-export const enums
export {
  EncryptionDirection,
  JobStatus,
  RejectReason,
} from '@graphene/sdk-native';

/**
 * Configuration options for the Graphene Client.
 */
export interface ClientConfig {
  /** Ed25519 secret key (32 bytes) */
  secretKey: Uint8Array;
  /** Worker's Ed25519 public key (32 bytes) */
  workerPubkey: Uint8Array;
  /** Solana payment channel PDA (32 bytes) */
  channelPda: Uint8Array;
  /** Worker's P2P node ID (hex string) */
  workerNodeId: string;
  /** Storage path for persistent data (default: '.graphene-sdk') */
  storagePath?: string;
  /** Whether to use relay servers for NAT traversal (default: true) */
  useRelay?: boolean;
  /** Optional bind port (0 for random) */
  bindPort?: number;
}

/**
 * Options for running a job.
 */
export interface RunOptions {
  /** Code to execute (will be encrypted) */
  code: string;
  /** Optional input data (will be encrypted) */
  input?: Uint8Array;
  /** Number of vCPUs (default: 1) */
  vcpu?: number;
  /** Memory in MB (default: 256) */
  memoryMb?: number;
  /** Execution timeout in milliseconds (default: 30000) */
  timeoutMs?: number;
  /** Kernel image to use (default: "python:3.12") */
  kernel?: string;
  /** Environment variables to pass to the job */
  env?: Record<string, string>;
  /** Allowed egress endpoints */
  egressAllowlist?: EgressRuleConfig[];
  /** Delivery mode: "sync" waits for result, "async" returns immediately */
  deliveryMode?: 'sync' | 'async';
}

/**
 * Simplified egress rule configuration.
 */
export interface EgressRuleConfig {
  /** Hostname or IP address */
  host: string;
  /** Port number */
  port: number;
  /** Protocol (default: "tcp") */
  protocol?: 'tcp' | 'udp';
}

/**
 * Result of a successful job execution.
 */
export interface RunResult {
  /** Exit code from the job (0 = success) */
  exitCode: number;
  /** Decrypted output data */
  output: Uint8Array;
  /** Execution duration in milliseconds */
  durationMs: number;
  /** Resource usage metrics */
  metrics: import('@graphene/sdk-native').JobMetrics;
}
