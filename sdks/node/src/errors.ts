/**
 * Custom error classes for the OpenCapsule SDK.
 *
 * @module errors
 */

/**
 * Valid reject reason values from the protocol.
 */
export type RejectReason =
  | 'TicketInvalid'
  | 'ChannelExhausted'
  | 'InsufficientPayment'
  | 'CapacityFull'
  | 'UnsupportedKernel'
  | 'ResourcesExceedLimits'
  | 'EnvTooLarge'
  | 'InvalidEnvName'
  | 'ReservedEnvPrefix'
  | 'AssetUnavailable'
  | 'InternalError';

/**
 * Base error class for all OpenCapsule SDK errors.
 */
export class OpenCapsuleError extends Error {
  /** Error code for programmatic handling */
  public readonly code: string;

  constructor(message: string, code: string) {
    super(message);
    this.name = 'OpenCapsuleError';
    this.code = code;
    // Maintains proper stack trace for where our error was thrown (only available on V8)
    if (Error.captureStackTrace) {
      Error.captureStackTrace(this, this.constructor);
    }
  }
}

/**
 * Error thrown when a job is rejected by the worker.
 */
export class JobRejectedError extends OpenCapsuleError {
  /** The reason the job was rejected */
  public readonly reason: RejectReason | string;

  constructor(reason: RejectReason | string, message?: string) {
    super(message ?? `Job rejected: ${reason}`, 'JOB_REJECTED');
    this.name = 'JobRejectedError';
    this.reason = reason;
  }
}

/**
 * Error thrown when a job execution fails (non-zero exit code).
 */
export class JobFailedError extends OpenCapsuleError {
  /** The exit code from the job */
  public readonly exitCode: number;
  /** The decrypted output (may contain error details) */
  public readonly output?: Uint8Array;

  constructor(exitCode: number, output?: Uint8Array, message?: string) {
    super(message ?? `Job failed with exit code ${exitCode}`, 'JOB_FAILED');
    this.name = 'JobFailedError';
    this.exitCode = exitCode;
    this.output = output;
  }
}

/**
 * Error thrown when a job times out.
 */
export class JobTimeoutError extends OpenCapsuleError {
  /** The timeout limit in milliseconds */
  public readonly timeoutMs: number;

  constructor(timeoutMs: number, message?: string) {
    super(message ?? `Job timed out after ${timeoutMs}ms`, 'JOB_TIMEOUT');
    this.name = 'JobTimeoutError';
    this.timeoutMs = timeoutMs;
  }
}

/**
 * Error thrown when transport operations fail.
 */
export class TransportError extends OpenCapsuleError {
  constructor(message: string) {
    super(message, 'TRANSPORT_ERROR');
    this.name = 'TransportError';
  }
}

/**
 * Error thrown when encryption or decryption fails.
 */
export class CryptoError extends OpenCapsuleError {
  constructor(message: string) {
    super(message, 'CRYPTO_ERROR');
    this.name = 'CryptoError';
  }
}

/**
 * Error thrown when payment ticket operations fail.
 */
export class PaymentError extends OpenCapsuleError {
  constructor(message: string) {
    super(message, 'PAYMENT_ERROR');
    this.name = 'PaymentError';
  }
}

/**
 * Error thrown when configuration is invalid.
 */
export class ConfigError extends OpenCapsuleError {
  constructor(message: string) {
    super(message, 'CONFIG_ERROR');
    this.name = 'ConfigError';
  }
}
