/**
 * Transport implementations for the Graphene SDK.
 *
 * @module transport
 */

import type { JobMetrics } from '@graphene/sdk-native';
import type { Transport, JobProgress } from './types.js';
import { TransportError } from './errors.js';

// String constants for JobStatus and RejectReason since const enums
// can't be accessed at runtime with isolatedModules
const JOB_STATUS_REJECTED = 'Rejected';
const JOB_STATUS_SUCCEEDED = 'Succeeded';
const REJECT_REASON_INTERNAL_ERROR = 'InternalError';

/**
 * Mock transport for testing purposes.
 *
 * Returns simulated successful responses without actual network communication.
 */
export class MockTransport implements Transport {
  private delay: number;
  private shouldFail: boolean;
  private failReason?: string;

  /**
   * Create a mock transport.
   *
   * @param options - Configuration options
   * @param options.delay - Simulated network delay in ms (default: 100)
   * @param options.shouldFail - Whether jobs should fail (default: false)
   * @param options.failReason - Reason for failure if shouldFail is true
   */
  constructor(options: {
    delay?: number;
    shouldFail?: boolean;
    failReason?: string;
  } = {}) {
    this.delay = options.delay ?? 100;
    this.shouldFail = options.shouldFail ?? false;
    this.failReason = options.failReason;
  }

  async send(_request: Uint8Array): Promise<Uint8Array> {
    // Simulate network delay
    await new Promise((resolve) => setTimeout(resolve, this.delay));

    // Decode the request to extract the job ID
    // Wire format: [4 bytes length BE] [1 byte type] [bincode payload]
    // We need to extract the job_id from the bincode payload
    // For simplicity in the mock, we'll generate a mock response

    // Create a mock response
    const mockJobId = '00000000-0000-0000-0000-000000000000';
    const mockMetrics: JobMetrics = {
      peakMemoryBytes: BigInt(128 * 1024 * 1024), // 128 MB
      cpuTimeMs: BigInt(500),
      networkRxBytes: BigInt(1024),
      networkTxBytes: BigInt(2048),
      totalCostMicros: BigInt(1000),
      cpuCostMicros: BigInt(500),
      memoryCostMicros: BigInt(300),
      egressCostMicros: BigInt(200),
    };

    if (this.shouldFail) {
      // Return a rejection response
      const response = {
        jobId: mockJobId,
        status: JOB_STATUS_REJECTED,
        rejectReason: this.failReason ?? REJECT_REASON_INTERNAL_ERROR,
        error: 'Mock transport configured to fail',
      };

      // Serialize to wire format manually for the mock
      // In reality, we'd need proper bincode serialization
      // For now, return a simple mock buffer that will be handled
      return this.createMockWireResponse(response);
    }

    // Return a successful response
    const response = {
      jobId: mockJobId,
      status: JOB_STATUS_SUCCEEDED,
      result: {
        resultHash: Buffer.alloc(32, 0xEE),
        exitCode: 0,
        durationMs: BigInt(500),
        metrics: mockMetrics,
        workerSignature: Buffer.alloc(64, 0xFF),
      },
    };

    return this.createMockWireResponse(response);
  }

  private createMockWireResponse(response: unknown): Uint8Array {
    // Create a simple mock wire format
    // In a real implementation, this would use proper bincode serialization
    // Wire format: [4 bytes length BE] [1 byte type=4 for JobResult] [JSON payload]
    // Convert BigInts to strings for JSON serialization
    const payload = Buffer.from(JSON.stringify(response, (_key, value) =>
      typeof value === 'bigint' ? value.toString() : value
    ));
    const wireMsg = Buffer.alloc(4 + 1 + payload.length);
    wireMsg.writeUInt32BE(1 + payload.length, 0);
    wireMsg[4] = 4; // JobResult type
    payload.copy(wireMsg, 5);
    return wireMsg;
  }

  onProgress?(_jobId: string, _callback: (progress: JobProgress) => void): void {
    // Mock doesn't support progress updates
  }

  async close(): Promise<void> {
    // Nothing to clean up in mock
  }
}

/**
 * Native Iroh transport for real network communication.
 *
 * Uses the native Rust bindings for QUIC-based communication with workers.
 */
export class IrohTransport implements Transport {
  private workerNodeId: string;
  private client: import('@graphene/sdk-native').GrapheneClient | null = null;
  private storagePath: string;
  private useRelay: boolean;

  /**
   * Create an Iroh transport.
   *
   * @param workerNodeId - The node ID (hex string) of the worker to connect to
   * @param options - Transport options
   * @param options.storagePath - Path for persistent storage (default: '.graphene-sdk')
   * @param options.useRelay - Whether to use relay servers for NAT traversal (default: true)
   */
  constructor(
    workerNodeId: string,
    options: { storagePath?: string; useRelay?: boolean } = {}
  ) {
    this.workerNodeId = workerNodeId;
    this.storagePath = options.storagePath ?? '.graphene-sdk';
    this.useRelay = options.useRelay ?? true;
  }

  private async ensureClient(): Promise<import('@graphene/sdk-native').GrapheneClient> {
    if (!this.client) {
      const { GrapheneClient } = await import('@graphene/sdk-native');
      this.client = await GrapheneClient.create({
        storagePath: this.storagePath,
        useRelay: this.useRelay,
      });
    }
    return this.client;
  }

  async send(request: Uint8Array): Promise<Uint8Array> {
    try {
      const client = await this.ensureClient();
      const response = await client.sendJobRequest(
        this.workerNodeId,
        Buffer.from(request)
      );
      return new Uint8Array(response);
    } catch (error) {
      throw new TransportError(
        `Failed to send job to worker ${this.workerNodeId}: ${error instanceof Error ? error.message : String(error)}`
      );
    }
  }

  onProgress?(_jobId: string, _callback: (progress: JobProgress) => void): void {
    // Progress streaming will be implemented when we add stream support
  }

  async close(): Promise<void> {
    if (this.client) {
      await this.client.shutdown();
      this.client = null;
    }
  }
}

/**
 * HTTP Gateway transport for development and testing.
 *
 * Sends jobs through an HTTP gateway that proxies to workers.
 * Useful when direct QUIC connectivity isn't available.
 */
export class HttpGatewayTransport implements Transport {
  private gatewayUrl: string;
  private headers: Record<string, string>;

  /**
   * Create an HTTP gateway transport.
   *
   * @param gatewayUrl - Base URL of the gateway (e.g., "https://gateway.graphene.network")
   * @param options - Additional options
   * @param options.headers - Additional headers to send with requests
   */
  constructor(
    gatewayUrl: string,
    options: { headers?: Record<string, string> } = {}
  ) {
    this.gatewayUrl = gatewayUrl.replace(/\/$/, ''); // Remove trailing slash
    this.headers = options.headers ?? {};
  }

  async send(request: Uint8Array): Promise<Uint8Array> {
    try {
      const response = await fetch(`${this.gatewayUrl}/v1/job`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/octet-stream',
          ...this.headers,
        },
        body: request,
      });

      if (!response.ok) {
        const errorText = await response.text();
        throw new TransportError(
          `Gateway returned ${response.status}: ${errorText}`
        );
      }

      const buffer = await response.arrayBuffer();
      return new Uint8Array(buffer);
    } catch (error) {
      if (error instanceof TransportError) {
        throw error;
      }
      throw new TransportError(
        `Failed to send job to gateway: ${error instanceof Error ? error.message : String(error)}`
      );
    }
  }

  async close(): Promise<void> {
    // HTTP is stateless, nothing to close
  }
}
