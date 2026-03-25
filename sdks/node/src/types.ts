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
  /** Shared channel identifier (32 bytes) - used for key derivation */
  channelId: Uint8Array;
  /** Worker's Ed25519 public key - hex-encoded (64 hex chars) */
  workerPubkey: string;
  /** Worker HTTP URL (e.g., "http://192.168.1.100:3000") */
  workerUrl: string;
}

/**
 * Resource requirements for a job.
 */
export interface ResourceOptions {
  /** Number of vCPUs (default: 1) */
  vcpu?: number;
  /** Memory in MB (default: 256) */
  memoryMb?: number;
}

/**
 * Networking options for a job.
 */
export interface NetworkingOptions {
  /** Estimated network ingress in megabytes */
  estimatedIngressMb?: number;
  /** Estimated network egress in megabytes */
  estimatedEgressMb?: number;
  /** Allowed egress endpoints */
  egressAllowlist?: EgressRuleConfig[];
}

/**
 * Options for running a job.
 */
export interface RunOptions {
  /** Code to execute (will be encrypted) */
  code: string;
  /** Optional input data (will be encrypted) */
  input?: Uint8Array;
  /** Resource requirements (vCPU, memory) */
  resources?: ResourceOptions;
  /** Networking options (egress allowlist, bandwidth estimates) */
  networking?: NetworkingOptions;
  /** Enable zstd compression for assets */
  compress?: boolean;
  /** Execution timeout in milliseconds (default: 30000) */
  timeoutMs?: number;
  /** Runtime image to use (default: "python:3.12") */
  runtime?: string;
  /** Environment variables to pass to the job */
  env?: Record<string, string>;
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
  metrics: {
    peakMemoryBytes: bigint;
    cpuTimeMs: bigint;
    networkRxBytes: bigint;
    networkTxBytes: bigint;
  };
}
