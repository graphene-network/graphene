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
  /** Solana payment channel PDA (32 bytes) */
  channelPda: Uint8Array;
  /** Worker's node ID - hex-encoded Ed25519 public key (64 hex chars) */
  workerNodeId: string;
  /** Storage path for persistent data (default: '.graphene-sdk') */
  storagePath?: string;
  /** Whether to use relay servers for NAT traversal (default: true) */
  useRelay?: boolean;
  /** Optional bind port (0 for random) */
  bindPort?: number;
  /** Worker's relay URL for NAT traversal (obtained from worker's connection info) */
  relayUrl?: string;
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
 * Asset delivery mode.
 *
 * - `auto` (default): Inline if under threshold, blob if over
 * - `inline`: Always inline, reject if over message limit (16 MB)
 * - `blob`: Always upload to Iroh first (for pre-staging, deduplication)
 */
export type AssetMode = 'auto' | 'inline' | 'blob';

/**
 * Options for asset delivery.
 */
export interface AssetOptions {
  /**
   * Delivery mode for assets.
   * - `auto` (default): Inline if under threshold, blob if over
   * - `inline`: Always inline, reject if over 16 MB message limit
   * - `blob`: Always upload to Iroh (for pre-staging, deduplication)
   */
  mode?: AssetMode;

  /**
   * Threshold for inline code in bytes (only for 'auto' mode).
   * Code larger than this will use blob mode.
   * @default 4194304 (4 MB)
   */
  inlineCodeThreshold?: number;

  /**
   * Threshold for inline input in bytes (only for 'auto' mode).
   * Input larger than this will use blob mode.
   * @default 8388608 (8 MB)
   */
  inlineInputThreshold?: number;

  /**
   * Enable zstd compression for assets before encryption.
   * Reduces payload size for compressible data.
   * @default false
   */
  compress?: boolean;

  /**
   * Additional files to include in the job.
   * Maps destination paths in the unikernel filesystem to local source paths.
   *
   * @example
   * ```typescript
   * files: {
   *   '/data/model.bin': './model.bin',
   *   '/config/settings.json': './config.json'
   * }
   * ```
   */
  files?: Record<string, string>;
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
  /** Asset delivery options (mode, compression, files) */
  assets?: AssetOptions;
  /** Execution timeout in milliseconds (default: 30000) */
  timeoutMs?: number;
  /** Kernel image to use (default: "python:3.12") */
  kernel?: string;
  /** Environment variables to pass to the job */
  env?: Record<string, string>;
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
