/**
 * Integration tests for the native SDK crypto bindings.
 * Run with: node --test tests/crypto.test.mjs
 */

import { test, describe } from 'node:test';
import assert from 'node:assert';
import { createPrivateKey, createPublicKey, generateKeyPairSync } from 'node:crypto';
import {
  deriveChannelKeys,
  encryptJobBlob,
  decryptJobBlob,
  EncryptedBlob,
  EncryptionDirection,
} from '../index.js';

// Helper to generate Ed25519 keypairs for testing
function generateEd25519Keypair(seed) {
  // Use seed bytes directly as the Ed25519 private key (32 bytes)
  const seedBuffer = Buffer.alloc(32);
  seedBuffer.fill(seed);
  return {
    secret: seedBuffer,
    // For Ed25519, we need to derive the public key
    // Since we're testing the SDK, we'll use a simple approach
  };
}

// Helper using node:crypto to generate proper Ed25519 keypairs
function generateEd25519KeypairProper() {
  const { privateKey, publicKey } = generateKeyPairSync('ed25519');

  // Export raw key bytes
  const privateKeyDer = privateKey.export({ type: 'pkcs8', format: 'der' });
  const publicKeyDer = publicKey.export({ type: 'spki', format: 'der' });

  // Extract raw 32-byte keys from DER encoding
  // Ed25519 private key in PKCS8: last 32 bytes after the 0x04 0x20 prefix in the octet string
  // Ed25519 public key in SPKI: last 32 bytes
  const secret = privateKeyDer.slice(-32);
  const pubkey = publicKeyDer.slice(-32);

  return { secret, pubkey };
}

describe('Channel Keys', () => {
  test('deriveChannelKeys creates valid keys', () => {
    const user = generateEd25519KeypairProper();
    const worker = generateEd25519KeypairProper();
    const channelPda = Buffer.alloc(32, 3);

    const keys = deriveChannelKeys(user.secret, worker.pubkey, channelPda);

    assert.ok(keys, 'Should return channel keys');
    assert.ok(keys.masterKey(), 'Should have master key');
    assert.strictEqual(keys.masterKey().length, 32, 'Master key should be 32 bytes');
    assert.ok(keys.peerPublicKey(), 'Should have peer public key');
    assert.strictEqual(keys.peerPublicKey().length, 32, 'Peer public key should be 32 bytes');
  });

  test('both parties derive same master key', () => {
    const user = generateEd25519KeypairProper();
    const worker = generateEd25519KeypairProper();
    const channelPda = Buffer.alloc(32, 3);

    const userKeys = deriveChannelKeys(user.secret, worker.pubkey, channelPda);
    const workerKeys = deriveChannelKeys(worker.secret, user.pubkey, channelPda);

    assert.deepStrictEqual(
      userKeys.masterKey(),
      workerKeys.masterKey(),
      'Both parties should derive same master key'
    );
  });

  test('different channels produce different keys', () => {
    const user = generateEd25519KeypairProper();
    const worker = generateEd25519KeypairProper();
    const channelPda1 = Buffer.alloc(32, 3);
    const channelPda2 = Buffer.alloc(32, 4);

    const keys1 = deriveChannelKeys(user.secret, worker.pubkey, channelPda1);
    const keys2 = deriveChannelKeys(user.secret, worker.pubkey, channelPda2);

    assert.notDeepStrictEqual(
      keys1.masterKey(),
      keys2.masterKey(),
      'Different channels should produce different keys'
    );
  });

  test('rejects invalid key lengths', () => {
    const validKey = Buffer.alloc(32, 1);
    const invalidKey = Buffer.alloc(31, 1); // Wrong length

    assert.throws(
      () => deriveChannelKeys(invalidKey, validKey, validKey),
      /Invalid key length/,
      'Should reject invalid secret key length'
    );

    assert.throws(
      () => deriveChannelKeys(validKey, invalidKey, validKey),
      /Invalid key length/,
      'Should reject invalid peer pubkey length'
    );

    assert.throws(
      () => deriveChannelKeys(validKey, validKey, invalidKey),
      /Invalid key length/,
      'Should reject invalid channel PDA length'
    );
  });
});

describe('Job Encryption', () => {
  test('encrypt and decrypt roundtrip', () => {
    const user = generateEd25519KeypairProper();
    const worker = generateEd25519KeypairProper();
    const channelPda = Buffer.alloc(32, 3);

    const userKeys = deriveChannelKeys(user.secret, worker.pubkey, channelPda);
    const workerKeys = deriveChannelKeys(worker.secret, user.pubkey, channelPda);

    const plaintext = Buffer.from('Hello, Graphene!');
    const jobId = 'test-job-123';

    // User encrypts input
    const encrypted = encryptJobBlob(plaintext, userKeys, jobId, EncryptionDirection.Input);

    assert.ok(encrypted, 'Should return encrypted blob');
    assert.strictEqual(encrypted.version, 1, 'Version should be 1');
    assert.strictEqual(encrypted.ephemeralPubkey.length, 32, 'Ephemeral pubkey should be 32 bytes');
    assert.strictEqual(encrypted.nonce.length, 24, 'Nonce should be 24 bytes');
    assert.ok(encrypted.ciphertext.length > 0, 'Ciphertext should not be empty');

    // Worker decrypts
    const decrypted = decryptJobBlob(encrypted, workerKeys, jobId, EncryptionDirection.Input);

    assert.deepStrictEqual(decrypted, plaintext, 'Decrypted should match plaintext');
  });

  test('bidirectional encryption works', () => {
    const user = generateEd25519KeypairProper();
    const worker = generateEd25519KeypairProper();
    const channelPda = Buffer.alloc(32, 3);

    const userKeys = deriveChannelKeys(user.secret, worker.pubkey, channelPda);
    const workerKeys = deriveChannelKeys(worker.secret, user.pubkey, channelPda);
    const jobId = 'test-job-456';

    // User encrypts input for worker
    const inputPlaintext = Buffer.from('User input data');
    const encryptedInput = encryptJobBlob(inputPlaintext, userKeys, jobId, EncryptionDirection.Input);
    const decryptedInput = decryptJobBlob(encryptedInput, workerKeys, jobId, EncryptionDirection.Input);
    assert.deepStrictEqual(decryptedInput, inputPlaintext);

    // Worker encrypts output for user
    const outputPlaintext = Buffer.from('Worker output data');
    const encryptedOutput = encryptJobBlob(outputPlaintext, workerKeys, jobId, EncryptionDirection.Output);
    const decryptedOutput = decryptJobBlob(encryptedOutput, userKeys, jobId, EncryptionDirection.Output);
    assert.deepStrictEqual(decryptedOutput, outputPlaintext);
  });

  test('wrong job ID fails decryption', () => {
    const user = generateEd25519KeypairProper();
    const worker = generateEd25519KeypairProper();
    const channelPda = Buffer.alloc(32, 3);

    const userKeys = deriveChannelKeys(user.secret, worker.pubkey, channelPda);
    const workerKeys = deriveChannelKeys(worker.secret, user.pubkey, channelPda);

    const plaintext = Buffer.from('Secret data');
    const encrypted = encryptJobBlob(plaintext, userKeys, 'job-1', EncryptionDirection.Input);

    assert.throws(
      () => decryptJobBlob(encrypted, workerKeys, 'job-2', EncryptionDirection.Input),
      /Decryption failed/,
      'Should fail with wrong job ID'
    );
  });

  test('wrong direction fails decryption', () => {
    const user = generateEd25519KeypairProper();
    const worker = generateEd25519KeypairProper();
    const channelPda = Buffer.alloc(32, 3);

    const userKeys = deriveChannelKeys(user.secret, worker.pubkey, channelPda);
    const workerKeys = deriveChannelKeys(worker.secret, user.pubkey, channelPda);

    const plaintext = Buffer.from('Secret data');
    const encrypted = encryptJobBlob(plaintext, userKeys, 'job-1', EncryptionDirection.Input);

    assert.throws(
      () => decryptJobBlob(encrypted, workerKeys, 'job-1', EncryptionDirection.Output),
      /Decryption failed/,
      'Should fail with wrong direction'
    );
  });
});

describe('EncryptedBlob Serialization', () => {
  test('toBytes and fromBytes roundtrip', () => {
    const user = generateEd25519KeypairProper();
    const worker = generateEd25519KeypairProper();
    const channelPda = Buffer.alloc(32, 3);

    const userKeys = deriveChannelKeys(user.secret, worker.pubkey, channelPda);
    const workerKeys = deriveChannelKeys(worker.secret, user.pubkey, channelPda);

    const plaintext = Buffer.from('Test data for serialization');
    const jobId = 'serialize-test';

    const original = encryptJobBlob(plaintext, userKeys, jobId, EncryptionDirection.Input);

    // Serialize and deserialize
    const bytes = original.toBytes();
    const restored = EncryptedBlob.fromBytes(bytes);

    // Verify structure
    assert.strictEqual(restored.version, original.version);
    assert.deepStrictEqual(restored.ephemeralPubkey, original.ephemeralPubkey);
    assert.deepStrictEqual(restored.nonce, original.nonce);
    assert.deepStrictEqual(restored.ciphertext, original.ciphertext);

    // Verify it can still be decrypted
    const decrypted = decryptJobBlob(restored, workerKeys, jobId, EncryptionDirection.Input);
    assert.deepStrictEqual(decrypted, plaintext);
  });

  test('fromBytes rejects invalid data', () => {
    // Too short
    assert.throws(
      () => EncryptedBlob.fromBytes(Buffer.alloc(10)),
      /too short/i,
      'Should reject blob that is too short'
    );

    // Wrong version
    const wrongVersion = Buffer.alloc(100, 0);
    wrongVersion[0] = 255; // Invalid version
    assert.throws(
      () => EncryptedBlob.fromBytes(wrongVersion),
      /version/i,
      'Should reject invalid version'
    );
  });
});
