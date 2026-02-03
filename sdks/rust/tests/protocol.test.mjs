/**
 * Integration tests for the native SDK protocol bindings.
 * Run with: node --test tests/protocol.test.mjs
 */

import { test, describe } from 'node:test';
import assert from 'node:assert';
import { randomBytes, generateKeyPairSync } from 'node:crypto';
import {
  serializeJobRequest,
  deserializeJobResponse,
  encodeWireMessage,
  decodeWireMessage,
  createPaymentTicket,
  JobStatus,
  RejectReason,
} from '../index.js';

// Helper to generate a proper Ed25519 keypair
function generateEd25519Keypair() {
  const { privateKey, publicKey } = generateKeyPairSync('ed25519');

  // Export raw key bytes
  const privateKeyDer = privateKey.export({ type: 'pkcs8', format: 'der' });
  const publicKeyDer = publicKey.export({ type: 'spki', format: 'der' });

  // Extract raw 32-byte keys from DER encoding
  const secret = privateKeyDer.slice(-32);
  const pubkey = publicKeyDer.slice(-32);

  return { secret, pubkey };
}

describe('Wire Message Encoding', () => {
  test('encodeWireMessage creates correct format', () => {
    const payload = Buffer.from('test payload');
    const msgType = 1; // JobRequest

    const encoded = encodeWireMessage(msgType, payload);

    // Check header: 4 bytes length + 1 byte type + payload
    assert.ok(encoded.length >= 5);

    // Check length field (big-endian)
    const length = encoded.readUInt32BE(0);
    assert.strictEqual(length, 1 + payload.length, 'Length should be type byte + payload length');

    // Check message type
    assert.strictEqual(encoded[4], msgType);

    // Check payload
    assert.deepStrictEqual(encoded.slice(5), payload);
  });

  test('decodeWireMessage parses correctly', () => {
    const payload = Buffer.from('hello world');
    const msgType = 3; // JobProgress

    const encoded = encodeWireMessage(msgType, payload);
    const decoded = decodeWireMessage(encoded);

    // Note: napi converts snake_case to camelCase
    assert.strictEqual(decoded.msgType, msgType);
    assert.deepStrictEqual(Buffer.from(decoded.payload), payload);
  });

  test('roundtrip encode/decode preserves data', () => {
    const originalPayload = Buffer.from(JSON.stringify({ test: 'data', number: 42 }));
    const msgType = 4; // JobResult

    const encoded = encodeWireMessage(msgType, originalPayload);
    const decoded = decodeWireMessage(encoded);

    assert.strictEqual(decoded.msgType, msgType);
    assert.deepStrictEqual(Buffer.from(decoded.payload), originalPayload);
  });

  test('rejects invalid message type', () => {
    const payload = Buffer.from('test');

    assert.throws(
      () => encodeWireMessage(0, payload),
      /Invalid message type/,
      'Should reject message type 0'
    );

    assert.throws(
      () => encodeWireMessage(6, payload),
      /Invalid message type/,
      'Should reject message type 6'
    );
  });

  test('decodeWireMessage rejects truncated data', () => {
    assert.throws(
      () => decodeWireMessage(Buffer.from([0, 0, 0])),
      /truncated/i,
      'Should reject data shorter than 5 bytes'
    );
  });

  test('decodeWireMessage rejects incomplete payload', () => {
    // Create a message that claims to be 100 bytes but only has 10
    const buf = Buffer.alloc(15);
    buf.writeUInt32BE(100, 0); // Length claims 100 bytes
    buf[4] = 1; // Message type

    assert.throws(
      () => decodeWireMessage(buf),
      /truncated/i,
      'Should reject incomplete payload'
    );
  });
});

describe('JobRequest Serialization', () => {
  test('serializeJobRequest creates valid wire format', () => {
    const signer = generateEd25519Keypair();
    const channelId = Buffer.alloc(32, 1);

    // Create a payment ticket
    const ticket = createPaymentTicket(
      channelId,
      BigInt(1000000), // 1 token
      BigInt(1), // nonce
      signer.secret
    );
    const ticketBytes = ticket.toBytes();

    // Note: napi converts snake_case to camelCase in JS
    // Use undefined (not null) for optional fields
    const request = {
      jobId: '550e8400-e29b-41d4-a716-446655440000',
      manifest: {
        vcpu: 2,
        memoryMb: 512,
        timeoutMs: BigInt(30000),
        kernel: 'python:3.12',
        egressAllowlist: [
          { host: 'api.example.com', port: 443, protocol: 'tcp' }
        ],
        env: { NODE_ENV: 'production' },
        estimatedEgressMb: BigInt(10),
      },
      ticket: ticketBytes,
      assets: {
        codeHash: Buffer.alloc(32, 0xAA),
        codeUrl: 'https://example.com/code.tar',
        inputHash: Buffer.alloc(32, 0xBB),
        // inputUrl is optional - omit it rather than null
      },
      ephemeralPubkey: Buffer.alloc(32, 0xCC),
      channelPda: Buffer.alloc(32, 0xDD),
      deliveryMode: 'sync',
    };

    const serialized = serializeJobRequest(request);

    // Verify wire format structure
    assert.ok(serialized.length >= 5, 'Should have at least header');

    // Decode the wire message to verify structure
    const decoded = decodeWireMessage(serialized);
    assert.strictEqual(decoded.msgType, 1, 'Should be JobRequest type (0x01)');
    assert.ok(decoded.payload.length > 0, 'Should have payload');
  });

  test('serializeJobRequest rejects invalid UUID', () => {
    const signer = generateEd25519Keypair();
    const ticket = createPaymentTicket(
      Buffer.alloc(32, 1),
      BigInt(1000000),
      BigInt(1),
      signer.secret
    );

    const request = {
      jobId: 'not-a-uuid',
      manifest: {
        vcpu: 1,
        memoryMb: 256,
        timeoutMs: BigInt(10000),
        kernel: 'python:3.12',
        egressAllowlist: [],
        env: {},
        // estimatedEgressMb is optional - omit it
      },
      ticket: ticket.toBytes(),
      assets: {
        codeHash: Buffer.alloc(32),
        // codeUrl is optional - omit it
        inputHash: Buffer.alloc(32),
        // inputUrl is optional - omit it
      },
      ephemeralPubkey: Buffer.alloc(32),
      channelPda: Buffer.alloc(32),
      deliveryMode: 'sync',
    };

    assert.throws(
      () => serializeJobRequest(request),
      /Invalid job_id UUID/,
      'Should reject invalid UUID format'
    );
  });

  test('serializeJobRequest rejects invalid key lengths', () => {
    const signer = generateEd25519Keypair();
    const ticket = createPaymentTicket(
      Buffer.alloc(32, 1),
      BigInt(1000000),
      BigInt(1),
      signer.secret
    );

    const baseRequest = {
      jobId: '550e8400-e29b-41d4-a716-446655440000',
      manifest: {
        vcpu: 1,
        memoryMb: 256,
        timeoutMs: BigInt(10000),
        kernel: 'python:3.12',
        egressAllowlist: [],
        env: {},
        // estimatedEgressMb is optional - omit it
      },
      ticket: ticket.toBytes(),
      assets: {
        codeHash: Buffer.alloc(32),
        // codeUrl is optional - omit it
        inputHash: Buffer.alloc(32),
        // inputUrl is optional - omit it
      },
      ephemeralPubkey: Buffer.alloc(32),
      channelPda: Buffer.alloc(32),
      deliveryMode: 'sync',
    };

    // Test invalid codeHash length
    assert.throws(
      () => serializeJobRequest({
        ...baseRequest,
        assets: { ...baseRequest.assets, codeHash: Buffer.alloc(31) }
      }),
      /code_hash must be 32 bytes/,
      'Should reject invalid codeHash length'
    );

    // Test invalid ephemeralPubkey length
    assert.throws(
      () => serializeJobRequest({
        ...baseRequest,
        ephemeralPubkey: Buffer.alloc(16)
      }),
      /ephemeral_pubkey must be 32 bytes/,
      'Should reject invalid ephemeralPubkey length'
    );
  });

  test('serializeJobRequest handles async delivery mode', () => {
    const signer = generateEd25519Keypair();
    const ticket = createPaymentTicket(
      Buffer.alloc(32, 1),
      BigInt(1000000),
      BigInt(1),
      signer.secret
    );

    const request = {
      jobId: '550e8400-e29b-41d4-a716-446655440000',
      manifest: {
        vcpu: 1,
        memoryMb: 256,
        timeoutMs: BigInt(10000),
        kernel: 'node:20',
        egressAllowlist: [],
        env: {},
        // estimatedEgressMb is optional - omit it
      },
      ticket: ticket.toBytes(),
      assets: {
        codeHash: Buffer.alloc(32),
        // codeUrl is optional - omit it
        inputHash: Buffer.alloc(32),
        // inputUrl is optional - omit it
      },
      ephemeralPubkey: Buffer.alloc(32),
      channelPda: Buffer.alloc(32),
      deliveryMode: 'async', // Test async mode
    };

    // Should not throw
    const serialized = serializeJobRequest(request);
    assert.ok(serialized.length > 0);
  });
});

describe('Enums Export', () => {
  test('JobStatus enum values are defined', () => {
    assert.ok(JobStatus, 'JobStatus should be exported');
    assert.strictEqual(JobStatus.Accepted, 'Accepted');
    assert.strictEqual(JobStatus.Running, 'Running');
    assert.strictEqual(JobStatus.Succeeded, 'Succeeded');
    assert.strictEqual(JobStatus.Failed, 'Failed');
    assert.strictEqual(JobStatus.Timeout, 'Timeout');
    assert.strictEqual(JobStatus.Rejected, 'Rejected');
  });

  test('RejectReason enum values are defined', () => {
    assert.ok(RejectReason, 'RejectReason should be exported');
    assert.strictEqual(RejectReason.TicketInvalid, 'TicketInvalid');
    assert.strictEqual(RejectReason.ChannelExhausted, 'ChannelExhausted');
    assert.strictEqual(RejectReason.InsufficientPayment, 'InsufficientPayment');
    assert.strictEqual(RejectReason.CapacityFull, 'CapacityFull');
    assert.strictEqual(RejectReason.UnsupportedKernel, 'UnsupportedKernel');
    assert.strictEqual(RejectReason.ResourcesExceedLimits, 'ResourcesExceedLimits');
    assert.strictEqual(RejectReason.EnvTooLarge, 'EnvTooLarge');
    assert.strictEqual(RejectReason.InvalidEnvName, 'InvalidEnvName');
    assert.strictEqual(RejectReason.ReservedEnvPrefix, 'ReservedEnvPrefix');
    assert.strictEqual(RejectReason.AssetUnavailable, 'AssetUnavailable');
    assert.strictEqual(RejectReason.InternalError, 'InternalError');
  });
});

describe('Wire Protocol Constants', () => {
  test('message types are in valid range', () => {
    // Valid message types: 1-5
    for (let msgType = 1; msgType <= 5; msgType++) {
      const payload = Buffer.from('test');
      const encoded = encodeWireMessage(msgType, payload);
      const decoded = decodeWireMessage(encoded);
      assert.strictEqual(decoded.msgType, msgType);
    }
  });

  test('large payload encoding works', () => {
    // Test with a larger payload (but not too large)
    const largePayload = Buffer.alloc(1024 * 100, 0x42); // 100KB
    const encoded = encodeWireMessage(1, largePayload);
    const decoded = decodeWireMessage(encoded);

    assert.strictEqual(decoded.msgType, 1);
    assert.strictEqual(decoded.payload.length, largePayload.length);
  });
});
