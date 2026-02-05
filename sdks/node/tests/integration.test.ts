/**
 * Integration tests for the Graphene SDK.
 *
 * These tests verify the TypeScript wrapper correctly delegates to native code.
 * Actual networking, crypto, and protocol tests are in Rust.
 */

import { describe, it, expect } from 'bun:test';
import {
  serializeJobRequest,
  deserializeJobResponse,
  createPaymentTicket,
  decodeWireMessage,
  encodeWireMessage,
} from '@graphene/sdk-native';

describe('Wire Protocol: Job Request Serialization', () => {
  it('serializes job request to wire format', () => {
    const channelPda = Buffer.alloc(32, 0x42);
    const secret = Buffer.alloc(32, 0x01);

    const ticket = createPaymentTicket(
      channelPda,
      BigInt(1000000),
      BigInt(1),
      secret
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
      ephemeralPubkey: Buffer.alloc(32, 0xcc),
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

  it('serializes with egress rules', () => {
    const channelPda = Buffer.alloc(32, 0x42);
    const secret = Buffer.alloc(32, 0x01);

    const ticket = createPaymentTicket(channelPda, BigInt(2000000), BigInt(2), secret);

    const request = {
      jobId: '550e8400-e29b-41d4-a716-446655440001',
      manifest: {
        vcpu: 2,
        memoryMb: 512,
        timeoutMs: BigInt(60000),
        kernel: 'node:21',
        egressAllowlist: [{ host: 'api.example.com', port: 443, protocol: 'tcp' }],
        env: { NODE_ENV: 'production' },
      },
      ticket: ticket.toBytes(),
      assets: {
        codeHash: Buffer.alloc(32, 0xcc),
        codeUrl: 'https://storage.example.com/code/abc123',
        inputHash: Buffer.alloc(32, 0xdd),
      },
      ephemeralPubkey: Buffer.alloc(32, 0xee),
      channelPda,
      deliveryMode: 'async',
    };

    const wireBytes = serializeJobRequest(request);
    const decoded = decodeWireMessage(wireBytes);

    expect(decoded.msgType).toBe(1);
    expect(decoded.payload.length).toBeGreaterThan(100);
  });
});

describe('Wire Protocol: Message Encoding', () => {
  it('encodes and decodes wire message', () => {
    const payload = Buffer.from('test payload data');
    const msgType = 1; // JobRequest

    const encoded = encodeWireMessage(msgType, payload);

    // Should have length prefix + type + payload
    expect(encoded.length).toBe(4 + 1 + payload.length);

    const decoded = decodeWireMessage(encoded);
    expect(decoded.msgType).toBe(msgType);
    expect(decoded.payload.toString()).toBe(payload.toString());
  });
});
