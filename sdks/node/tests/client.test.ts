/**
 * Tests for the Graphene SDK client.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { generateKeyPairSync } from 'node:crypto';
import {
  Client,
  MockTransport,
  GrapheneError,
  JobRejectedError,
  ConfigError,
  CryptoError,
} from '../src/index.js';
import type { Transport, JobProgress } from '../src/types.js';

/**
 * Helper to generate Ed25519 keypairs for testing.
 */
function generateEd25519Keypair(): { secret: Buffer; pubkey: Buffer } {
  const { privateKey, publicKey } = generateKeyPairSync('ed25519');

  // Export raw key bytes from DER encoding
  const privateKeyDer = privateKey.export({ type: 'pkcs8', format: 'der' });
  const publicKeyDer = publicKey.export({ type: 'spki', format: 'der' });

  // Extract raw 32-byte keys (last 32 bytes of DER)
  const secret = privateKeyDer.slice(-32) as Buffer;
  const pubkey = publicKeyDer.slice(-32) as Buffer;

  return { secret, pubkey };
}

describe('Client', () => {
  let userKeys: { secret: Buffer; pubkey: Buffer };
  let workerKeys: { secret: Buffer; pubkey: Buffer };
  let channelPda: Buffer;

  beforeEach(() => {
    userKeys = generateEd25519Keypair();
    workerKeys = generateEd25519Keypair();
    channelPda = Buffer.alloc(32, 0x42);
  });

  describe('constructor', () => {
    it('creates client with valid config', () => {
      const client = new Client({
        secretKey: userKeys.secret,
        workerPubkey: workerKeys.pubkey,
        channelPda,
      });

      expect(client).toBeInstanceOf(Client);
      expect(client.currentNonce).toBe(0n);
      expect(client.totalAuthorized).toBe(0n);
    });

    it('rejects invalid secretKey length', () => {
      expect(() => {
        new Client({
          secretKey: Buffer.alloc(31),
          workerPubkey: workerKeys.pubkey,
          channelPda,
        });
      }).toThrow(ConfigError);
    });

    it('rejects invalid workerPubkey length', () => {
      expect(() => {
        new Client({
          secretKey: userKeys.secret,
          workerPubkey: Buffer.alloc(16),
          channelPda,
        });
      }).toThrow(ConfigError);
    });

    it('rejects invalid channelPda length', () => {
      expect(() => {
        new Client({
          secretKey: userKeys.secret,
          workerPubkey: workerKeys.pubkey,
          channelPda: Buffer.alloc(64),
        });
      }).toThrow(ConfigError);
    });

    it('accepts Uint8Array inputs', () => {
      const client = new Client({
        secretKey: new Uint8Array(userKeys.secret),
        workerPubkey: new Uint8Array(workerKeys.pubkey),
        channelPda: new Uint8Array(channelPda),
      });

      expect(client).toBeInstanceOf(Client);
    });
  });

  describe('encrypt/decrypt', () => {
    it('encrypts and decrypts data roundtrip', () => {
      const client = new Client({
        secretKey: userKeys.secret,
        workerPubkey: workerKeys.pubkey,
        channelPda,
      });

      const workerClient = new Client({
        secretKey: workerKeys.secret,
        workerPubkey: userKeys.pubkey,
        channelPda,
      });

      const jobId = 'test-job-123';
      const plaintext = new TextEncoder().encode('secret message');

      // User encrypts for worker
      const encrypted = client.encrypt(plaintext, jobId, 'input');
      expect(encrypted).toBeInstanceOf(Uint8Array);
      expect(encrypted.length).toBeGreaterThan(plaintext.length);

      // Worker decrypts
      const decrypted = workerClient.decrypt(encrypted, jobId, 'input');
      expect(new TextDecoder().decode(decrypted)).toBe('secret message');
    });

    it('fails decryption with wrong job ID', () => {
      const client = new Client({
        secretKey: userKeys.secret,
        workerPubkey: workerKeys.pubkey,
        channelPda,
      });

      const workerClient = new Client({
        secretKey: workerKeys.secret,
        workerPubkey: userKeys.pubkey,
        channelPda,
      });

      const plaintext = new TextEncoder().encode('secret');
      const encrypted = client.encrypt(plaintext, 'job-1', 'input');

      expect(() => {
        workerClient.decrypt(encrypted, 'job-2', 'input');
      }).toThrow(CryptoError);
    });

    it('fails decryption with wrong direction', () => {
      const client = new Client({
        secretKey: userKeys.secret,
        workerPubkey: workerKeys.pubkey,
        channelPda,
      });

      const workerClient = new Client({
        secretKey: workerKeys.secret,
        workerPubkey: userKeys.pubkey,
        channelPda,
      });

      const plaintext = new TextEncoder().encode('secret');
      const encrypted = client.encrypt(plaintext, 'job-1', 'input');

      expect(() => {
        workerClient.decrypt(encrypted, 'job-1', 'output');
      }).toThrow(CryptoError);
    });

    it('supports bidirectional encryption', () => {
      const client = new Client({
        secretKey: userKeys.secret,
        workerPubkey: workerKeys.pubkey,
        channelPda,
      });

      const workerClient = new Client({
        secretKey: workerKeys.secret,
        workerPubkey: userKeys.pubkey,
        channelPda,
      });

      const jobId = 'bidirectional-test';

      // User encrypts input for worker
      const inputData = new TextEncoder().encode('user input');
      const encryptedInput = client.encrypt(inputData, jobId, 'input');
      const decryptedInput = workerClient.decrypt(encryptedInput, jobId, 'input');
      expect(new TextDecoder().decode(decryptedInput)).toBe('user input');

      // Worker encrypts output for user
      const outputData = new TextEncoder().encode('worker output');
      const encryptedOutput = workerClient.encrypt(outputData, jobId, 'output');
      const decryptedOutput = client.decrypt(encryptedOutput, jobId, 'output');
      expect(new TextDecoder().decode(decryptedOutput)).toBe('worker output');
    });
  });

  describe('close', () => {
    it('closes the transport', async () => {
      const mockClose = vi.fn().mockResolvedValue(undefined);
      const transport: Transport = {
        send: vi.fn().mockResolvedValue(new Uint8Array()),
        close: mockClose,
      };

      const client = new Client({
        secretKey: userKeys.secret,
        workerPubkey: workerKeys.pubkey,
        channelPda,
        transport,
      });

      await client.close();
      expect(mockClose).toHaveBeenCalledOnce();
    });
  });

  describe('state tracking', () => {
    it('tracks nonce correctly', () => {
      const client = new Client({
        secretKey: userKeys.secret,
        workerPubkey: workerKeys.pubkey,
        channelPda,
      });

      expect(client.currentNonce).toBe(0n);
    });

    it('tracks total authorized amount', () => {
      const client = new Client({
        secretKey: userKeys.secret,
        workerPubkey: workerKeys.pubkey,
        channelPda,
      });

      expect(client.totalAuthorized).toBe(0n);
    });
  });
});

describe('MockTransport', () => {
  it('returns response data', async () => {
    const transport = new MockTransport();
    const response = await transport.send(new Uint8Array(100));
    expect(response).toBeInstanceOf(Uint8Array);
  });

  it('simulates delay', async () => {
    const transport = new MockTransport({ delay: 50 });
    const start = Date.now();
    await transport.send(new Uint8Array(100));
    const elapsed = Date.now() - start;
    expect(elapsed).toBeGreaterThanOrEqual(40); // Allow some timing variance
  });

  it('can be configured to fail', async () => {
    const transport = new MockTransport({
      shouldFail: true,
      failReason: 'CapacityFull',
    });
    const response = await transport.send(new Uint8Array(100));
    // Response should be a Uint8Array (mock wire message)
    expect(response).toBeInstanceOf(Uint8Array);
  });

  it('close does nothing', async () => {
    const transport = new MockTransport();
    await expect(transport.close()).resolves.toBeUndefined();
  });
});

describe('Error classes', () => {
  it('GrapheneError has code property', () => {
    const error = new GrapheneError('test', 'TEST_CODE');
    expect(error.code).toBe('TEST_CODE');
    expect(error.message).toBe('test');
    expect(error.name).toBe('GrapheneError');
  });

  it('JobRejectedError has reason property', () => {
    const error = new JobRejectedError('CapacityFull');
    expect(error.reason).toBe('CapacityFull');
    expect(error.code).toBe('JOB_REJECTED');
    expect(error.message).toContain('CapacityFull');
  });

  it('JobRejectedError accepts custom message', () => {
    const error = new JobRejectedError('TicketInvalid', 'Custom error message');
    expect(error.reason).toBe('TicketInvalid');
    expect(error.message).toBe('Custom error message');
  });

  it('ConfigError identifies configuration issues', () => {
    const error = new ConfigError('Invalid key length');
    expect(error.code).toBe('CONFIG_ERROR');
    expect(error.name).toBe('ConfigError');
  });

  it('CryptoError identifies crypto issues', () => {
    const error = new CryptoError('Decryption failed');
    expect(error.code).toBe('CRYPTO_ERROR');
    expect(error.name).toBe('CryptoError');
  });
});
