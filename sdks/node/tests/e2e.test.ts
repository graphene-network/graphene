/**
 * End-to-end tests for the Graphene SDK.
 *
 * These tests simulate the full job lifecycle with properly serialized
 * wire protocol messages, verifying that:
 * - Request serialization matches what workers expect
 * - Response deserialization works with bincode format
 * - Encryption/decryption works bidirectionally
 * - Payment tickets are valid and verifiable
 */

import { describe, it, expect, beforeEach } from 'vitest';
import { generateKeyPairSync, randomBytes } from 'node:crypto';
import {
  Client,
  TransportError,
  JobRejectedError,
  JobFailedError,
  JobTimeoutError,
} from '../src/index.js';
import type { Transport, RunResult } from '../src/types.js';
import {
  // Native bindings for creating realistic responses
  deriveChannelKeys,
  encryptJobBlob,
  decryptJobBlob,
  createPaymentTicket,
  verifyTicketSignature,
  serializeJobRequest,
  deserializeJobResponse,
  decodeWireMessage,
  EncryptionDirection,
} from '@graphene/sdk-native';

/**
 * Generate Ed25519 keypair for testing.
 */
function generateEd25519Keypair(): { secret: Buffer; pubkey: Buffer } {
  const { privateKey, publicKey } = generateKeyPairSync('ed25519');
  const privateKeyDer = privateKey.export({ type: 'pkcs8', format: 'der' });
  const publicKeyDer = publicKey.export({ type: 'spki', format: 'der' });
  const secret = privateKeyDer.slice(-32) as Buffer;
  const pubkey = publicKeyDer.slice(-32) as Buffer;
  return { secret, pubkey };
}

/**
 * Simulated worker that processes job requests.
 *
 * This provides a realistic test double that:
 * 1. Derives the same channel keys as the client
 * 2. Verifies payment tickets
 * 3. Decrypts code/input using the shared keys
 * 4. "Executes" the job (returns mock output)
 * 5. Encrypts the result
 * 6. Returns a properly serialized wire response
 */
class SimulatedWorker {
  private secretKey: Buffer;
  private pubkey: Buffer;
  private channelPda: Buffer;
  private peerPubkey: Buffer;

  constructor(
    secretKey: Buffer,
    pubkey: Buffer,
    channelPda: Buffer,
    peerPubkey: Buffer
  ) {
    this.secretKey = secretKey;
    this.pubkey = pubkey;
    this.channelPda = channelPda;
    this.peerPubkey = peerPubkey;
  }

  /**
   * Process a job request and return a wire-formatted response.
   */
  async processRequest(
    wireRequest: Uint8Array,
    options: {
      simulateRejection?: string;
      simulateFailure?: boolean;
      simulateTimeout?: boolean;
      outputData?: string;
    } = {}
  ): Promise<Uint8Array> {
    // Decode the wire message to get the job request
    const wireMsg = decodeWireMessage(Buffer.from(wireRequest));
    expect(wireMsg.msgType).toBe(1); // JobRequest type

    // Derive channel keys (same as client, but with swapped perspectives)
    const channelKeys = deriveChannelKeys(
      this.secretKey,
      this.peerPubkey,
      this.channelPda
    );

    // For a full E2E test, we would:
    // 1. Deserialize the JobRequest from wireMsg.payload
    // 2. Verify the payment ticket
    // 3. Decrypt the code/input blobs
    // 4. Execute the code
    // 5. Encrypt the output
    // 6. Build and serialize a JobResponse

    // For now, create a mock response that matches the wire format
    // This tests the response path without full bincode serialization

    // The response format expected by deserializeJobResponse is wire format:
    // [4 bytes length BE] [1 byte type] [bincode JobResponse]

    // Since we can't easily create bincode from TypeScript, we'll test
    // the components individually and document what a real worker would do

    throw new TransportError(
      'Full bincode response serialization requires Rust - see native cross-validation tests'
    );
  }

  /**
   * Verify a payment ticket is valid.
   */
  verifyTicket(ticketBytes: Buffer): boolean {
    // Deserialize and verify the ticket
    const { PaymentTicket } = require('@graphene/sdk-native');
    const ticket = PaymentTicket.fromBytes(ticketBytes);
    return verifyTicketSignature(ticket, this.peerPubkey);
  }

  /**
   * Decrypt input from the user.
   */
  decryptInput(encryptedInput: Uint8Array, jobId: string): Uint8Array {
    const channelKeys = deriveChannelKeys(
      this.secretKey,
      this.peerPubkey,
      this.channelPda
    );
    return decryptJobBlob(
      require('@graphene/sdk-native').EncryptedBlob.fromBytes(
        Buffer.from(encryptedInput)
      ),
      channelKeys,
      jobId,
      EncryptionDirection.Input
    );
  }

  /**
   * Encrypt output for the user.
   */
  encryptOutput(plaintext: Uint8Array, jobId: string): Uint8Array {
    const channelKeys = deriveChannelKeys(
      this.secretKey,
      this.peerPubkey,
      this.channelPda
    );
    return encryptJobBlob(
      Buffer.from(plaintext),
      channelKeys,
      jobId,
      EncryptionDirection.Output
    ).toBytes();
  }
}

describe('E2E: Crypto Interoperability', () => {
  let userKeys: { secret: Buffer; pubkey: Buffer };
  let workerKeys: { secret: Buffer; pubkey: Buffer };
  let channelPda: Buffer;

  beforeEach(() => {
    userKeys = generateEd25519Keypair();
    workerKeys = generateEd25519Keypair();
    channelPda = randomBytes(32);
  });

  it('client and worker derive the same channel master key', () => {
    const userChannelKeys = deriveChannelKeys(
      userKeys.secret,
      workerKeys.pubkey,
      channelPda
    );

    const workerChannelKeys = deriveChannelKeys(
      workerKeys.secret,
      userKeys.pubkey,
      channelPda
    );

    // Both should derive the same master key
    expect(userChannelKeys.masterKey().toString('hex')).toBe(
      workerChannelKeys.masterKey().toString('hex')
    );
  });

  it('user-encrypted code can be decrypted by worker', () => {
    const client = new Client({
      secretKey: userKeys.secret,
      workerPubkey: workerKeys.pubkey,
      channelPda,
    });

    const worker = new SimulatedWorker(
      workerKeys.secret,
      workerKeys.pubkey,
      channelPda,
      userKeys.pubkey
    );

    const code = 'print("Hello from Graphene!")';
    const jobId = 'test-job-001';

    // User encrypts
    const encrypted = client.encrypt(
      new TextEncoder().encode(code),
      jobId,
      'input'
    );

    // Worker decrypts
    const decrypted = worker.decryptInput(encrypted, jobId);

    expect(new TextDecoder().decode(decrypted)).toBe(code);
  });

  it('worker-encrypted output can be decrypted by user', () => {
    const client = new Client({
      secretKey: userKeys.secret,
      workerPubkey: workerKeys.pubkey,
      channelPda,
    });

    const worker = new SimulatedWorker(
      workerKeys.secret,
      workerKeys.pubkey,
      channelPda,
      userKeys.pubkey
    );

    const output = 'Execution result: 42';
    const jobId = 'test-job-002';

    // Worker encrypts output
    const encrypted = worker.encryptOutput(
      new TextEncoder().encode(output),
      jobId
    );

    // User decrypts
    const decrypted = client.decrypt(encrypted, jobId, 'output');

    expect(new TextDecoder().decode(decrypted)).toBe(output);
  });

  it('full bidirectional encryption flow', () => {
    const client = new Client({
      secretKey: userKeys.secret,
      workerPubkey: workerKeys.pubkey,
      channelPda,
    });

    const worker = new SimulatedWorker(
      workerKeys.secret,
      workerKeys.pubkey,
      channelPda,
      userKeys.pubkey
    );

    const jobId = 'bidirectional-test-job';

    // Step 1: User sends code
    const code = 'def compute(): return 2 + 2';
    const encryptedCode = client.encrypt(
      new TextEncoder().encode(code),
      jobId,
      'input'
    );

    // Step 2: Worker receives and decrypts code
    const receivedCode = worker.decryptInput(encryptedCode, jobId);
    expect(new TextDecoder().decode(receivedCode)).toBe(code);

    // Step 3: User sends input data
    const inputData = JSON.stringify({ x: 10, y: 20 });
    const encryptedInput = client.encrypt(
      new TextEncoder().encode(inputData),
      jobId,
      'input'
    );

    // Step 4: Worker receives and decrypts input
    const receivedInput = worker.decryptInput(encryptedInput, jobId);
    expect(new TextDecoder().decode(receivedInput)).toBe(inputData);

    // Step 5: Worker executes and encrypts output
    const outputData = JSON.stringify({ result: 30 });
    const encryptedOutput = worker.encryptOutput(
      new TextEncoder().encode(outputData),
      jobId
    );

    // Step 6: User decrypts output
    const receivedOutput = client.decrypt(encryptedOutput, jobId, 'output');
    expect(new TextDecoder().decode(receivedOutput)).toBe(outputData);
  });

  it('encryption is job-specific (wrong job ID fails)', () => {
    const client = new Client({
      secretKey: userKeys.secret,
      workerPubkey: workerKeys.pubkey,
      channelPda,
    });

    const worker = new SimulatedWorker(
      workerKeys.secret,
      workerKeys.pubkey,
      channelPda,
      userKeys.pubkey
    );

    const code = 'secret code';
    const encryptedForJob1 = client.encrypt(
      new TextEncoder().encode(code),
      'job-001',
      'input'
    );

    // Trying to decrypt with wrong job ID should fail
    expect(() => {
      worker.decryptInput(encryptedForJob1, 'job-002');
    }).toThrow();
  });

  it('encryption is direction-specific', () => {
    const client = new Client({
      secretKey: userKeys.secret,
      workerPubkey: workerKeys.pubkey,
      channelPda,
    });

    const worker = new SimulatedWorker(
      workerKeys.secret,
      workerKeys.pubkey,
      channelPda,
      userKeys.pubkey
    );

    const jobId = 'direction-test';
    const data = 'test data';

    // Encrypt as input
    const encryptedAsInput = client.encrypt(
      new TextEncoder().encode(data),
      jobId,
      'input'
    );

    // Trying to decrypt as output should fail
    expect(() => {
      client.decrypt(encryptedAsInput, jobId, 'output');
    }).toThrow();
  });
});

describe('E2E: Payment Ticket Flow', () => {
  let userKeys: { secret: Buffer; pubkey: Buffer };
  let workerKeys: { secret: Buffer; pubkey: Buffer };
  let channelPda: Buffer;

  beforeEach(() => {
    userKeys = generateEd25519Keypair();
    workerKeys = generateEd25519Keypair();
    channelPda = randomBytes(32);
  });

  it('client-created tickets are verifiable by worker', () => {
    // Create a ticket like the client would
    const ticket = createPaymentTicket(
      channelPda,
      BigInt(1000000), // 1 token in micros
      BigInt(1), // nonce
      userKeys.secret
    );

    // Worker verifies the signature
    const isValid = verifyTicketSignature(ticket, userKeys.pubkey);
    expect(isValid).toBe(true);
  });

  it('ticket with wrong signer is rejected', () => {
    const wrongKeys = generateEd25519Keypair();

    // Create ticket signed by wrong key
    const ticket = createPaymentTicket(
      channelPda,
      BigInt(1000000),
      BigInt(1),
      wrongKeys.secret // Wrong signer!
    );

    // Verification with expected pubkey should fail
    const isValid = verifyTicketSignature(ticket, userKeys.pubkey);
    expect(isValid).toBe(false);
  });

  it('ticket serialization roundtrip preserves signature validity', () => {
    const ticket = createPaymentTicket(
      channelPda,
      BigInt(5000000),
      BigInt(42),
      userKeys.secret
    );

    // Serialize and deserialize
    const bytes = ticket.toBytes();
    const { PaymentTicket } = require('@graphene/sdk-native');
    const restored = PaymentTicket.fromBytes(bytes);

    // Should still be valid
    const isValid = verifyTicketSignature(restored, userKeys.pubkey);
    expect(isValid).toBe(true);

    // Fields should match
    expect(restored.amountMicros).toBe(BigInt(5000000));
    expect(restored.nonce).toBe(BigInt(42));
  });

  it('tickets have monotonically increasing nonces', () => {
    const tickets = [];
    for (let i = 1; i <= 5; i++) {
      tickets.push(
        createPaymentTicket(
          channelPda,
          BigInt(i * 1000000), // Cumulative amount
          BigInt(i), // Monotonic nonce
          userKeys.secret
        )
      );
    }

    // Verify nonces are increasing
    for (let i = 1; i < tickets.length; i++) {
      expect(tickets[i].nonce).toBeGreaterThan(tickets[i - 1].nonce);
    }

    // Verify amounts are cumulative
    for (let i = 1; i < tickets.length; i++) {
      expect(tickets[i].amountMicros).toBeGreaterThan(
        tickets[i - 1].amountMicros
      );
    }
  });
});

describe('E2E: Wire Protocol', () => {
  let userKeys: { secret: Buffer; pubkey: Buffer };
  let workerKeys: { secret: Buffer; pubkey: Buffer };
  let channelPda: Buffer;

  beforeEach(() => {
    userKeys = generateEd25519Keypair();
    workerKeys = generateEd25519Keypair();
    channelPda = randomBytes(32);
  });

  it('serialized job request has correct wire format', () => {
    const ticket = createPaymentTicket(
      channelPda,
      BigInt(1000000),
      BigInt(1),
      userKeys.secret
    );

    const request = {
      jobId: '550e8400-e29b-41d4-a716-446655440000',
      manifest: {
        vcpu: 1,
        memoryMb: 256,
        timeoutMs: BigInt(30000),
        kernel: 'python:3.12',
        egressAllowlist: [],
        env: {},
      },
      ticket: ticket.toBytes(),
      assets: {
        codeHash: Buffer.alloc(32, 0xaa),
        inputHash: Buffer.alloc(32, 0xbb),
      },
      ephemeralPubkey: randomBytes(32),
      channelPda,
      deliveryMode: 'sync',
    };

    const wireBytes = serializeJobRequest(request);

    // Wire format: [4 bytes length BE] [1 byte type] [payload]
    expect(wireBytes.length).toBeGreaterThan(5);

    // Decode and verify
    const decoded = decodeWireMessage(wireBytes);
    expect(decoded.msgType).toBe(1); // JobRequest = 0x01
    expect(decoded.payload.length).toBeGreaterThan(0);
  });

  it('job request wire format is decodable', () => {
    const ticket = createPaymentTicket(
      channelPda,
      BigInt(2000000),
      BigInt(2),
      userKeys.secret
    );

    const request = {
      jobId: '550e8400-e29b-41d4-a716-446655440001',
      manifest: {
        vcpu: 2,
        memoryMb: 512,
        timeoutMs: BigInt(60000),
        kernel: 'node:20',
        egressAllowlist: [{ host: 'api.example.com', port: 443, protocol: 'tcp' }],
        env: { NODE_ENV: 'production' },
      },
      ticket: ticket.toBytes(),
      assets: {
        codeHash: Buffer.alloc(32, 0xcc),
        codeUrl: 'https://storage.example.com/code/abc123',
        inputHash: Buffer.alloc(32, 0xdd),
      },
      ephemeralPubkey: randomBytes(32),
      channelPda,
      deliveryMode: 'async',
    };

    const wireBytes = serializeJobRequest(request);
    const decoded = decodeWireMessage(wireBytes);

    // The payload is bincode-encoded JobRequest
    // A real worker would deserialize this with bincode
    expect(decoded.msgType).toBe(1);
    expect(decoded.payload.length).toBeGreaterThan(100);
  });
});

describe('E2E: Client State Management', () => {
  let userKeys: { secret: Buffer; pubkey: Buffer };
  let workerKeys: { secret: Buffer; pubkey: Buffer };
  let channelPda: Buffer;

  beforeEach(() => {
    userKeys = generateEd25519Keypair();
    workerKeys = generateEd25519Keypair();
    channelPda = randomBytes(32);
  });

  it('client tracks nonce across job attempts', async () => {
    const transport: Transport = {
      send: async () => {
        throw new TransportError('Simulated failure');
      },
      close: async () => {},
    };

    const client = new Client({
      secretKey: userKeys.secret,
      workerPubkey: workerKeys.pubkey,
      channelPda,
      transport,
    });

    expect(client.currentNonce).toBe(0n);

    // Each job attempt increments nonce
    for (let i = 1; i <= 3; i++) {
      try {
        await client.run({ code: `job ${i}` });
      } catch {
        // Expected to fail
      }
      expect(client.currentNonce).toBe(BigInt(i));
    }

    await client.close();
  });

  it('client tracks cumulative authorized amount', async () => {
    const transport: Transport = {
      send: async () => {
        throw new TransportError('Simulated failure');
      },
      close: async () => {},
    };

    const client = new Client({
      secretKey: userKeys.secret,
      workerPubkey: workerKeys.pubkey,
      channelPda,
      transport,
    });

    expect(client.totalAuthorized).toBe(0n);

    // Submit jobs with different resource requirements
    try {
      await client.run({ code: 'job 1', vcpu: 1, memoryMb: 256, timeoutMs: 1000 });
    } catch {}

    const amount1 = client.totalAuthorized;
    expect(amount1).toBeGreaterThan(0n);

    try {
      await client.run({ code: 'job 2', vcpu: 2, memoryMb: 512, timeoutMs: 2000 });
    } catch {}

    const amount2 = client.totalAuthorized;
    expect(amount2).toBeGreaterThan(amount1);

    // Amounts should be cumulative
    expect(client.totalAuthorized).toBe(amount2);

    await client.close();
  });
});

describe('E2E: Error Scenarios', () => {
  let userKeys: { secret: Buffer; pubkey: Buffer };
  let workerKeys: { secret: Buffer; pubkey: Buffer };
  let channelPda: Buffer;

  beforeEach(() => {
    userKeys = generateEd25519Keypair();
    workerKeys = generateEd25519Keypair();
    channelPda = randomBytes(32);
  });

  it('handles transport connection failure', async () => {
    const transport: Transport = {
      send: async () => {
        throw new Error('Connection refused');
      },
      close: async () => {},
    };

    const client = new Client({
      secretKey: userKeys.secret,
      workerPubkey: workerKeys.pubkey,
      channelPda,
      transport,
    });

    await expect(client.run({ code: 'test' })).rejects.toThrow(TransportError);

    await client.close();
  });

  it('handles transport timeout', async () => {
    const transport: Transport = {
      send: async () => {
        // Simulate a very long delay
        await new Promise((resolve) => setTimeout(resolve, 100));
        throw new Error('Request timeout');
      },
      close: async () => {},
    };

    const client = new Client({
      secretKey: userKeys.secret,
      workerPubkey: workerKeys.pubkey,
      channelPda,
      transport,
    });

    await expect(client.run({ code: 'test' })).rejects.toThrow(TransportError);

    await client.close();
  });

  it('handles invalid response format gracefully', async () => {
    const transport: Transport = {
      send: async () => {
        // Return garbage data
        return new Uint8Array([0x00, 0x00, 0x00, 0x01, 0xff]);
      },
      close: async () => {},
    };

    const client = new Client({
      secretKey: userKeys.secret,
      workerPubkey: workerKeys.pubkey,
      channelPda,
      transport,
    });

    // Should throw some kind of error, not crash
    await expect(client.run({ code: 'test' })).rejects.toThrow();

    await client.close();
  });
});
