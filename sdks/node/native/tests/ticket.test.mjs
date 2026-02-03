/**
 * Integration tests for the native SDK payment ticket bindings.
 * Run with: node --test tests/ticket.test.mjs
 */

import { test, describe } from 'node:test';
import assert from 'node:assert';
import { generateKeyPairSync } from 'node:crypto';
import {
  PaymentTicket,
  createPaymentTicket,
  verifyTicketSignature,
  validateTicket,
} from '../index.js';

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

describe('Payment Ticket Creation', () => {
  test('createPaymentTicket creates a valid ticket', () => {
    const { secret, pubkey } = generateEd25519KeypairProper();
    const channelId = Buffer.alloc(32, 1);
    const amountMicros = 1000000n;
    const nonce = 1n;

    const ticket = createPaymentTicket(channelId, amountMicros, nonce, secret);

    assert.ok(ticket, 'Should return a ticket');
    assert.deepStrictEqual(ticket.channelId, channelId, 'Channel ID should match');
    assert.strictEqual(ticket.amountMicros, amountMicros, 'Amount should match');
    assert.strictEqual(ticket.nonce, nonce, 'Nonce should match');
    assert.ok(ticket.timestamp > 0, 'Timestamp should be positive');
    assert.strictEqual(ticket.signature().length, 64, 'Signature should be 64 bytes');
  });

  test('createPaymentTicket signature is verifiable', () => {
    const { secret, pubkey } = generateEd25519KeypairProper();
    const channelId = Buffer.alloc(32, 2);
    const amountMicros = 5000000n;
    const nonce = 5n;

    const ticket = createPaymentTicket(channelId, amountMicros, nonce, secret);
    const isValid = verifyTicketSignature(ticket, pubkey);

    assert.strictEqual(isValid, true, 'Signature should be valid');
  });

  test('createPaymentTicket rejects invalid key lengths', () => {
    const validChannelId = Buffer.alloc(32, 1);
    const invalidChannelId = Buffer.alloc(31, 1);
    const validSecret = Buffer.alloc(32, 1);
    const invalidSecret = Buffer.alloc(31, 1);

    assert.throws(
      () => createPaymentTicket(invalidChannelId, 1n, 1n, validSecret),
      /Invalid key length/,
      'Should reject invalid channel ID length'
    );

    assert.throws(
      () => createPaymentTicket(validChannelId, 1n, 1n, invalidSecret),
      /Invalid key length/,
      'Should reject invalid secret key length'
    );
  });
});

describe('Ticket Signature Verification', () => {
  test('verifyTicketSignature returns true for valid signature', () => {
    const { secret, pubkey } = generateEd25519KeypairProper();
    const channelId = Buffer.alloc(32, 3);

    const ticket = createPaymentTicket(channelId, 1000n, 1n, secret);
    const isValid = verifyTicketSignature(ticket, pubkey);

    assert.strictEqual(isValid, true, 'Valid signature should verify');
  });

  test('verifyTicketSignature returns false for wrong public key', () => {
    const signer = generateEd25519KeypairProper();
    const wrongKey = generateEd25519KeypairProper();
    const channelId = Buffer.alloc(32, 4);

    const ticket = createPaymentTicket(channelId, 1000n, 1n, signer.secret);
    const isValid = verifyTicketSignature(ticket, wrongKey.pubkey);

    assert.strictEqual(isValid, false, 'Wrong public key should fail verification');
  });

  test('verifyTicketSignature returns false for invalid public key bytes', () => {
    const { secret } = generateEd25519KeypairProper();
    const channelId = Buffer.alloc(32, 5);
    const invalidPubkey = Buffer.alloc(32, 0xFF); // Not a valid curve point

    const ticket = createPaymentTicket(channelId, 1000n, 1n, secret);
    const isValid = verifyTicketSignature(ticket, invalidPubkey);

    assert.strictEqual(isValid, false, 'Invalid public key should fail verification');
  });

  test('verifyTicketSignature rejects wrong pubkey length', () => {
    const { secret } = generateEd25519KeypairProper();
    const channelId = Buffer.alloc(32, 6);
    const wrongLengthPubkey = Buffer.alloc(31, 1);

    const ticket = createPaymentTicket(channelId, 1000n, 1n, secret);

    assert.throws(
      () => verifyTicketSignature(ticket, wrongLengthPubkey),
      /Invalid key length/,
      'Should reject wrong pubkey length'
    );
  });
});

describe('Ticket Serialization', () => {
  test('toBytes and fromBytes roundtrip', () => {
    const { secret, pubkey } = generateEd25519KeypairProper();
    const channelId = Buffer.alloc(32, 7);
    const amountMicros = 9999999n;
    const nonce = 42n;

    const original = createPaymentTicket(channelId, amountMicros, nonce, secret);
    const bytes = original.toBytes();
    const restored = PaymentTicket.fromBytes(bytes);

    assert.deepStrictEqual(restored.channelId, original.channelId, 'Channel ID should match');
    assert.strictEqual(restored.amountMicros, original.amountMicros, 'Amount should match');
    assert.strictEqual(restored.nonce, original.nonce, 'Nonce should match');
    assert.strictEqual(restored.timestamp, original.timestamp, 'Timestamp should match');
    assert.deepStrictEqual(restored.signature(), original.signature(), 'Signature should match');

    // Verify restored ticket signature still works
    const isValid = verifyTicketSignature(restored, pubkey);
    assert.strictEqual(isValid, true, 'Restored ticket signature should be valid');
  });

  test('fromBytes rejects invalid data', () => {
    assert.throws(
      () => PaymentTicket.fromBytes(Buffer.alloc(10)),
      /Deserialization failed/,
      'Should reject invalid ticket bytes'
    );
  });
});

describe('Ticket Validation', () => {
  test('validateTicket accepts valid ticket', async () => {
    const { secret, pubkey } = generateEd25519KeypairProper();
    const channelId = Buffer.alloc(32, 8);
    const amountMicros = 1000000n;
    const nonce = 5n;

    const ticket = createPaymentTicket(channelId, amountMicros, nonce, secret);

    const channelState = {
      lastNonce: 4n,           // Previous nonce was 4, ticket nonce is 5
      lastAmount: 500000n,     // Previous amount was 500000, ticket is 1000000
      channelBalance: 10000000n, // Channel has 10M balance
    };

    // Should not throw
    await validateTicket(ticket, pubkey, channelState);
  });

  test('validateTicket rejects invalid signature', async () => {
    const signer = generateEd25519KeypairProper();
    const wrongKey = generateEd25519KeypairProper();
    const channelId = Buffer.alloc(32, 9);

    const ticket = createPaymentTicket(channelId, 1000000n, 5n, signer.secret);

    const channelState = {
      lastNonce: 4n,
      lastAmount: 500000n,
      channelBalance: 10000000n,
    };

    await assert.rejects(
      async () => validateTicket(ticket, wrongKey.pubkey, channelState),
      /invalid signature/i,
      'Should reject invalid signature'
    );
  });

  test('validateTicket rejects replayed nonce', async () => {
    const { secret, pubkey } = generateEd25519KeypairProper();
    const channelId = Buffer.alloc(32, 10);

    const ticket = createPaymentTicket(channelId, 1000000n, 5n, secret);

    const channelState = {
      lastNonce: 5n,           // Same as ticket nonce - should be rejected
      lastAmount: 500000n,
      channelBalance: 10000000n,
    };

    await assert.rejects(
      async () => validateTicket(ticket, pubkey, channelState),
      /replayed nonce/i,
      'Should reject replayed nonce'
    );
  });

  test('validateTicket rejects nonce less than last seen', async () => {
    const { secret, pubkey } = generateEd25519KeypairProper();
    const channelId = Buffer.alloc(32, 11);

    const ticket = createPaymentTicket(channelId, 1000000n, 5n, secret);

    const channelState = {
      lastNonce: 10n,          // Last nonce was 10, ticket nonce is 5
      lastAmount: 500000n,
      channelBalance: 10000000n,
    };

    await assert.rejects(
      async () => validateTicket(ticket, pubkey, channelState),
      /replayed nonce/i,
      'Should reject nonce less than last seen'
    );
  });

  test('validateTicket rejects non-cumulative amount', async () => {
    const { secret, pubkey } = generateEd25519KeypairProper();
    const channelId = Buffer.alloc(32, 12);

    const ticket = createPaymentTicket(channelId, 500000n, 5n, secret);

    const channelState = {
      lastNonce: 4n,
      lastAmount: 1000000n,    // Last amount was 1M, ticket is only 500K
      channelBalance: 10000000n,
    };

    await assert.rejects(
      async () => validateTicket(ticket, pubkey, channelState),
      /non-cumulative amount/i,
      'Should reject non-cumulative amount'
    );
  });

  test('validateTicket rejects amount exceeding balance', async () => {
    const { secret, pubkey } = generateEd25519KeypairProper();
    const channelId = Buffer.alloc(32, 13);

    const ticket = createPaymentTicket(channelId, 20000000n, 5n, secret);

    const channelState = {
      lastNonce: 4n,
      lastAmount: 500000n,
      channelBalance: 10000000n,  // Only 10M balance, ticket wants 20M
    };

    await assert.rejects(
      async () => validateTicket(ticket, pubkey, channelState),
      /insufficient balance/i,
      'Should reject amount exceeding channel balance'
    );
  });

  test('validateTicket rejects invalid pubkey length', async () => {
    const { secret } = generateEd25519KeypairProper();
    const channelId = Buffer.alloc(32, 14);
    const invalidPubkey = Buffer.alloc(31, 1);

    const ticket = createPaymentTicket(channelId, 1000000n, 5n, secret);

    const channelState = {
      lastNonce: 4n,
      lastAmount: 500000n,
      channelBalance: 10000000n,
    };

    await assert.rejects(
      async () => validateTicket(ticket, invalidPubkey, channelState),
      /Invalid key length/,
      'Should reject invalid pubkey length'
    );
  });
});

describe('Edge Cases', () => {
  test('handles large amounts (u64 max)', () => {
    const { secret, pubkey } = generateEd25519KeypairProper();
    const channelId = Buffer.alloc(32, 15);
    const largeAmount = 18446744073709551615n; // u64::MAX

    const ticket = createPaymentTicket(channelId, largeAmount, 1n, secret);

    assert.strictEqual(ticket.amountMicros, largeAmount, 'Should handle u64 max amount');

    const isValid = verifyTicketSignature(ticket, pubkey);
    assert.strictEqual(isValid, true, 'Signature should be valid for large amounts');
  });

  test('handles large nonces', () => {
    const { secret, pubkey } = generateEd25519KeypairProper();
    const channelId = Buffer.alloc(32, 16);
    const largeNonce = 18446744073709551615n; // u64::MAX

    const ticket = createPaymentTicket(channelId, 1000n, largeNonce, secret);

    assert.strictEqual(ticket.nonce, largeNonce, 'Should handle u64 max nonce');

    const isValid = verifyTicketSignature(ticket, pubkey);
    assert.strictEqual(isValid, true, 'Signature should be valid for large nonces');
  });

  test('different channel IDs produce different signatures', () => {
    const { secret } = generateEd25519KeypairProper();
    const channelId1 = Buffer.alloc(32, 17);
    const channelId2 = Buffer.alloc(32, 18);

    const ticket1 = createPaymentTicket(channelId1, 1000n, 1n, secret);
    const ticket2 = createPaymentTicket(channelId2, 1000n, 1n, secret);

    assert.notDeepStrictEqual(
      ticket1.signature(),
      ticket2.signature(),
      'Different channel IDs should produce different signatures'
    );
  });
});
