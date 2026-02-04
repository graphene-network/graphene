/**
 * Test key utilities for E2E tests.
 *
 * Generates Ed25519 keypairs for testing payment ticket signing.
 */

import { randomBytes } from 'crypto';

// @noble/ed25519 is a minimal Ed25519 implementation
// We use dynamic import since bun test should resolve it
let ed25519: typeof import('@noble/ed25519') | null = null;

async function getEd25519() {
  if (!ed25519) {
    ed25519 = await import('@noble/ed25519');
  }
  return ed25519;
}

/**
 * An Ed25519 keypair for testing.
 */
export interface TestKeypair {
  /** 32-byte secret key */
  secretKey: Uint8Array;
  /** 32-byte public key */
  publicKey: Uint8Array;
  /** Public key as hex string (for env vars) */
  publicKeyHex: string;
}

/**
 * Generate a random Ed25519 keypair for testing.
 */
export async function generateTestKeypair(): Promise<TestKeypair> {
  const ed = await getEd25519();

  // Generate random 32-byte secret key
  const secretKey = new Uint8Array(randomBytes(32));

  // Derive public key from secret key
  const publicKey = await ed.getPublicKeyAsync(secretKey);

  // Convert public key to hex string
  const publicKeyHex = Buffer.from(publicKey).toString('hex');

  return {
    secretKey,
    publicKey,
    publicKeyHex,
  };
}

/**
 * Create a deterministic test keypair from a seed string.
 * Useful for reproducible tests.
 */
export async function keypairFromSeed(seed: string): Promise<TestKeypair> {
  const ed = await getEd25519();

  // Hash the seed to get a deterministic 32-byte secret key
  const { createHash } = await import('crypto');
  const secretKey = new Uint8Array(createHash('sha256').update(seed).digest());

  // Derive public key from secret key
  const publicKey = await ed.getPublicKeyAsync(secretKey);

  // Convert public key to hex string
  const publicKeyHex = Buffer.from(publicKey).toString('hex');

  return {
    secretKey,
    publicKey,
    publicKeyHex,
  };
}

/**
 * Generate a test channel PDA (32 bytes of 0x01).
 * This matches the hardcoded test channel in server.rs.
 */
export function testChannelPda(): Uint8Array {
  return new Uint8Array(32).fill(0x01);
}
