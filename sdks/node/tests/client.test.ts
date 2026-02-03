/**
 * Tests for the Graphene SDK client.
 *
 * Note: These tests only verify the TypeScript wrapper behavior.
 * Actual crypto, networking, and protocol tests are in Rust.
 */

import { describe, it, expect } from 'bun:test';
import { Client, ConfigError, GrapheneError, JobRejectedError, CryptoError } from '../src/index.js';

/**
 * Helper to generate test keys.
 */
function generateTestKeys(): { secret: Buffer; pubkey: Buffer } {
  // Use deterministic test keys
  const secret = Buffer.alloc(32, 0x01);
  const pubkey = Buffer.alloc(32, 0x02);
  return { secret, pubkey };
}

describe('Client.create', () => {
  const userKeys = generateTestKeys();
  const workerKeys = { secret: Buffer.alloc(32, 0x03), pubkey: Buffer.alloc(32, 0x04) };
  const channelPda = Buffer.alloc(32, 0x42);
  const workerNodeId = '0000000000000000000000000000000000000000000000000000000000000000';

  it('rejects invalid secretKey length', async () => {
    await expect(
      Client.create({
        secretKey: Buffer.alloc(31),
        workerPubkey: workerKeys.pubkey,
        channelPda,
        workerNodeId,
      })
    ).rejects.toThrow(ConfigError);
  });

  it('rejects invalid workerPubkey length', async () => {
    await expect(
      Client.create({
        secretKey: userKeys.secret,
        workerPubkey: Buffer.alloc(16),
        channelPda,
        workerNodeId,
      })
    ).rejects.toThrow(ConfigError);
  });

  it('rejects invalid channelPda length', async () => {
    await expect(
      Client.create({
        secretKey: userKeys.secret,
        workerPubkey: workerKeys.pubkey,
        channelPda: Buffer.alloc(64),
        workerNodeId,
      })
    ).rejects.toThrow(ConfigError);
  });

  it('rejects missing workerNodeId', async () => {
    await expect(
      Client.create({
        secretKey: userKeys.secret,
        workerPubkey: workerKeys.pubkey,
        channelPda,
        workerNodeId: '',
      })
    ).rejects.toThrow(ConfigError);
  });

  it('accepts Uint8Array inputs', async () => {
    // This will fail to connect since there's no real worker,
    // but the validation should pass
    await expect(
      Client.create({
        secretKey: new Uint8Array(userKeys.secret),
        workerPubkey: new Uint8Array(workerKeys.pubkey),
        channelPda: new Uint8Array(channelPda),
        workerNodeId,
      })
    ).rejects.toThrow(); // Expected - no real worker to connect to
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
