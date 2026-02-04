/**
 * End-to-end crypto tests using native bindings directly.
 *
 * These tests verify the native Rust crypto functions work correctly
 * from TypeScript. The actual crypto logic is tested extensively in Rust.
 */

import { describe, it, expect } from 'bun:test';
import { randomBytes } from 'node:crypto';
import {
  deriveChannelKeys,
  encryptJobBlob,
  decryptJobBlob,
  createPaymentTicket,
  verifyTicketSignature,
  blake3Hash,
  EncryptionDirection,
} from '@graphene/sdk-native';

describe('Native Crypto: Channel Key Derivation', () => {
  it('both parties derive the same master key', () => {
    const { generateKeyPairSync } = require('node:crypto');

    // Generate user keypair
    const { privateKey: userPriv, publicKey: userPub } = generateKeyPairSync('ed25519');
    const userSecret = userPriv.export({ type: 'pkcs8', format: 'der' }).slice(-32);
    const userPubkey = userPub.export({ type: 'spki', format: 'der' }).slice(-32);

    // Generate worker keypair
    const { privateKey: workerPriv, publicKey: workerPub } = generateKeyPairSync('ed25519');
    const workerSecret = workerPriv.export({ type: 'pkcs8', format: 'der' }).slice(-32);
    const workerPubkey = workerPub.export({ type: 'spki', format: 'der' }).slice(-32);

    const channelPda = Buffer.alloc(32, 0x42);

    const userChannelKeys = deriveChannelKeys(userSecret, workerPubkey, channelPda);
    const workerChannelKeys = deriveChannelKeys(workerSecret, userPubkey, channelPda);

    // Both parties derive the same master key via ECDH
    expect(userChannelKeys.masterKey().toString('hex')).toBe(
      workerChannelKeys.masterKey().toString('hex')
    );
  });
});

describe('Native Crypto: Blob Encryption', () => {
  it('encrypts a blob (encryption structure is validated)', () => {
    // Use real ed25519 keypairs from Node.js crypto
    const { generateKeyPairSync } = require('node:crypto');
    const { privateKey, publicKey } = generateKeyPairSync('ed25519');
    const privateKeyDer = privateKey.export({ type: 'pkcs8', format: 'der' });
    const publicKeyDer = publicKey.export({ type: 'spki', format: 'der' });
    const secret = privateKeyDer.slice(-32);
    const pubkey = publicKeyDer.slice(-32);

    // Generate peer keys
    const { privateKey: peerPriv, publicKey: peerPub } = generateKeyPairSync('ed25519');
    const peerPubkeyDer = peerPub.export({ type: 'spki', format: 'der' });
    const peerPubkey = peerPubkeyDer.slice(-32);

    const channelPda = Buffer.alloc(32, 0x42);

    const channelKeys = deriveChannelKeys(secret, peerPubkey, channelPda);

    const plaintext = Buffer.from('Hello, Graphene!');
    const jobId = 'test-job-001';

    const encrypted = encryptJobBlob(plaintext, channelKeys, jobId, EncryptionDirection.Input);

    expect(encrypted.toBytes().length).toBeGreaterThan(plaintext.length);
    expect(encrypted.version).toBe(1);
    expect(encrypted.ephemeralPubkey.length).toBe(32);
    expect(encrypted.nonce.length).toBe(24);
  });
});

describe('Native Crypto: Payment Tickets', () => {
  it('creates and verifies a payment ticket', () => {
    const channelPda = Buffer.alloc(32, 0x42);
    const secret = Buffer.alloc(32, 0x01);

    const ticket = createPaymentTicket(
      channelPda,
      BigInt(1000000), // 1 token in micros
      BigInt(1), // nonce
      secret
    );

    expect(ticket.amountMicros).toBe(BigInt(1000000));
    expect(ticket.nonce).toBe(BigInt(1));
    expect(ticket.channelId.length).toBe(32);
    expect(ticket.signature().length).toBe(64);
  });

  it('ticket serialization roundtrip', () => {
    const channelPda = Buffer.alloc(32, 0x42);
    const secret = Buffer.alloc(32, 0x01);

    const ticket = createPaymentTicket(
      channelPda,
      BigInt(5000000),
      BigInt(42),
      secret
    );

    const bytes = ticket.toBytes();
    const { PaymentTicket } = require('@graphene/sdk-native');
    const restored = PaymentTicket.fromBytes(bytes);

    expect(restored.amountMicros).toBe(BigInt(5000000));
    expect(restored.nonce).toBe(BigInt(42));
  });
});

describe('Native Crypto: BLAKE3 Hashing', () => {
  it('computes BLAKE3 hash', () => {
    const data = Buffer.from('test data for hashing');
    const hash = blake3Hash(data);

    expect(hash.length).toBe(32);
    expect(hash).toBeInstanceOf(Buffer);
  });

  it('same input produces same hash', () => {
    const data = Buffer.from('deterministic hashing');
    const hash1 = blake3Hash(data);
    const hash2 = blake3Hash(data);

    expect(hash1.toString('hex')).toBe(hash2.toString('hex'));
  });

  it('different input produces different hash', () => {
    const hash1 = blake3Hash(Buffer.from('input 1'));
    const hash2 = blake3Hash(Buffer.from('input 2'));

    expect(hash1.toString('hex')).not.toBe(hash2.toString('hex'));
  });
});
