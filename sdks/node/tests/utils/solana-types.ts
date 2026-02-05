/**
 * Solana types and instruction builders for Graphene program interaction.
 *
 * These utilities allow direct program interaction without requiring the full
 * Anchor IDL, using raw instruction data and account parsing.
 */

import { readFileSync } from 'fs';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';
import { Keypair, PublicKey, TransactionInstruction, SYSVAR_INSTRUCTIONS_PUBKEY } from '@solana/web3.js';
import { TOKEN_PROGRAM_ID } from '@solana/spl-token';
import { createHash } from 'crypto';

// Ed25519 program for signature verification
export const ED25519_PROGRAM_ID = new PublicKey('Ed25519SigVerify111111111111111111111111111');

// Graphene program ID (derived from graphene-keypair.json if available)
const DEFAULT_GRAPHENE_PROGRAM_ID = new PublicKey('3yErVeGSU3LHZzTnKjkoV5fPkcFQxyjeroLRo5VtSvEf');

function loadProgramId(): PublicKey {
  try {
    const currentDir = dirname(fileURLToPath(import.meta.url));
    const keypairPath = join(
      currentDir,
      '../../../../programs/graphene/target/deploy/graphene-keypair.json'
    );
    const raw = JSON.parse(readFileSync(keypairPath, 'utf8')) as number[];
    if (Array.isArray(raw) && raw.length >= 64) {
      const secretKey = Uint8Array.from(raw);
      return Keypair.fromSecretKey(secretKey).publicKey;
    }
  } catch {
    // Fall back to the default ID below.
  }
  return DEFAULT_GRAPHENE_PROGRAM_ID;
}

export const GRAPHENE_PROGRAM_ID = loadProgramId();

/**
 * Anchor discriminators (first 8 bytes of sha256("global:<method_name>"))
 */
function computeDiscriminator(name: string): Buffer {
  const hash = createHash('sha256').update(`global:${name}`).digest();
  return hash.slice(0, 8);
}

export const DISCRIMINATORS = {
  openChannel: computeDiscriminator('open_channel'),
  topUpChannel: computeDiscriminator('top_up_channel'),
  settleChannel: computeDiscriminator('settle_channel'),
  initiateClose: computeDiscriminator('initiate_close'),
  forceClose: computeDiscriminator('force_close'),
  cooperativeClose: computeDiscriminator('cooperative_close'),
} as const;

/**
 * PaymentChannel account discriminator (first 8 bytes of sha256("account:PaymentChannel"))
 */
export const PAYMENT_CHANNEL_DISCRIMINATOR = createHash('sha256')
  .update('account:PaymentChannel')
  .digest()
  .slice(0, 8);

/**
 * Channel state enum values
 */
export enum ChannelState {
  Open = 0,
  Closing = 1,
}

/**
 * Parsed PaymentChannel account data
 */
export interface ParsedPaymentChannel {
  user: PublicKey;
  worker: PublicKey;
  mint: PublicKey;
  balance: bigint;
  spent: bigint;
  lastNonce: bigint;
  timeout: bigint;
  state: ChannelState;
  bump: number;
}

/**
 * Parse a PaymentChannel account's raw data.
 *
 * Account layout (138 bytes):
 * - 0-8: discriminator
 * - 8-40: user pubkey
 * - 40-72: worker pubkey
 * - 72-104: mint pubkey
 * - 104-112: balance (u64 LE)
 * - 112-120: spent (u64 LE)
 * - 120-128: last_nonce (u64 LE)
 * - 128-136: timeout (i64 LE)
 * - 136: state (1 byte)
 * - 137: bump (1 byte)
 */
export function parsePaymentChannel(data: Buffer): ParsedPaymentChannel {
  if (data.length !== 138) {
    throw new Error(`Invalid PaymentChannel data length: expected 138, got ${data.length}`);
  }

  // Verify discriminator
  const discriminator = data.slice(0, 8);
  if (!discriminator.equals(PAYMENT_CHANNEL_DISCRIMINATOR)) {
    throw new Error('Invalid PaymentChannel discriminator');
  }

  return {
    user: new PublicKey(data.slice(8, 40)),
    worker: new PublicKey(data.slice(40, 72)),
    mint: new PublicKey(data.slice(72, 104)),
    balance: data.readBigUInt64LE(104),
    spent: data.readBigUInt64LE(112),
    lastNonce: data.readBigUInt64LE(120),
    timeout: data.readBigInt64LE(128),
    state: data.readUInt8(136) as ChannelState,
    bump: data.readUInt8(137),
  };
}

/**
 * Derive the channel PDA address.
 */
export function deriveChannelPda(
  user: PublicKey,
  worker: PublicKey,
  programId: PublicKey = GRAPHENE_PROGRAM_ID
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [Buffer.from('channel'), user.toBuffer(), worker.toBuffer()],
    programId
  );
}

/**
 * Derive the vault PDA address for a channel.
 */
export function deriveVaultPda(
  channelPda: PublicKey,
  programId: PublicKey = GRAPHENE_PROGRAM_ID
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [Buffer.from('vault'), channelPda.toBuffer()],
    programId
  );
}

/**
 * Build the instruction data for open_channel.
 *
 * @param amount - Initial deposit amount in token base units
 */
export function buildOpenChannelData(amount: bigint): Buffer {
  const data = Buffer.alloc(8 + 8); // discriminator + u64
  DISCRIMINATORS.openChannel.copy(data, 0);
  data.writeBigUInt64LE(amount, 8);
  return data;
}

/**
 * Build the instruction data for top_up_channel.
 *
 * @param amount - Top-up amount in token base units
 */
export function buildTopUpChannelData(amount: bigint): Buffer {
  const data = Buffer.alloc(8 + 8); // discriminator + u64
  DISCRIMINATORS.topUpChannel.copy(data, 0);
  data.writeBigUInt64LE(amount, 8);
  return data;
}

/**
 * Build the instruction data for settle_channel.
 *
 * @param amount - Cumulative spent amount
 * @param nonce - Ticket nonce
 */
export function buildSettleChannelData(amount: bigint, nonce: bigint): Buffer {
  const data = Buffer.alloc(8 + 8 + 8); // discriminator + u64 + u64
  DISCRIMINATORS.settleChannel.copy(data, 0);
  data.writeBigUInt64LE(amount, 8);
  data.writeBigUInt64LE(nonce, 16);
  return data;
}

/**
 * Build the instruction data for initiate_close.
 */
export function buildInitiateCloseData(): Buffer {
  return Buffer.from(DISCRIMINATORS.initiateClose);
}

/**
 * Build the instruction data for cooperative_close.
 *
 * @param finalSpent - Final spent amount to settle
 */
export function buildCooperativeCloseData(finalSpent: bigint): Buffer {
  const data = Buffer.alloc(8 + 8); // discriminator + u64
  DISCRIMINATORS.cooperativeClose.copy(data, 0);
  data.writeBigUInt64LE(finalSpent, 8);
  return data;
}

/**
 * Build an open_channel instruction.
 */
export function buildOpenChannelInstruction(params: {
  user: PublicKey;
  worker: PublicKey;
  channelPda: PublicKey;
  mint: PublicKey;
  userTokenAccount: PublicKey;
  vaultPda: PublicKey;
  amount: bigint;
  programId?: PublicKey;
}): TransactionInstruction {
  const {
    user,
    worker,
    channelPda,
    mint,
    userTokenAccount,
    vaultPda,
    amount,
    programId = GRAPHENE_PROGRAM_ID,
  } = params;

  return new TransactionInstruction({
    programId,
    keys: [
      { pubkey: user, isSigner: true, isWritable: true },
      { pubkey: worker, isSigner: false, isWritable: false },
      { pubkey: channelPda, isSigner: false, isWritable: true },
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: userTokenAccount, isSigner: false, isWritable: true },
      { pubkey: vaultPda, isSigner: false, isWritable: true },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
      { pubkey: new PublicKey('11111111111111111111111111111111'), isSigner: false, isWritable: false },
    ],
    data: buildOpenChannelData(amount),
  });
}

/**
 * Build a top_up_channel instruction.
 */
export function buildTopUpChannelInstruction(params: {
  user: PublicKey;
  channelPda: PublicKey;
  mint: PublicKey;
  userTokenAccount: PublicKey;
  vaultPda: PublicKey;
  amount: bigint;
  programId?: PublicKey;
}): TransactionInstruction {
  const {
    user,
    channelPda,
    mint,
    userTokenAccount,
    vaultPda,
    amount,
    programId = GRAPHENE_PROGRAM_ID,
  } = params;

  return new TransactionInstruction({
    programId,
    keys: [
      { pubkey: user, isSigner: true, isWritable: false },
      { pubkey: channelPda, isSigner: false, isWritable: true },
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: userTokenAccount, isSigner: false, isWritable: true },
      { pubkey: vaultPda, isSigner: false, isWritable: true },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
    ],
    data: buildTopUpChannelData(amount),
  });
}

/**
 * Build an initiate_close instruction.
 */
export function buildInitiateCloseInstruction(params: {
  user: PublicKey;
  channelPda: PublicKey;
  programId?: PublicKey;
}): TransactionInstruction {
  const { user, channelPda, programId = GRAPHENE_PROGRAM_ID } = params;

  return new TransactionInstruction({
    programId,
    keys: [
      { pubkey: user, isSigner: true, isWritable: false },
      { pubkey: channelPda, isSigner: false, isWritable: true },
    ],
    data: buildInitiateCloseData(),
  });
}

/**
 * Build an Ed25519 signature verification instruction for payment tickets.
 *
 * This creates the Ed25519 program instruction that must precede the settle_channel
 * instruction in the same transaction.
 *
 * @param signature - 64-byte Ed25519 signature
 * @param publicKey - 32-byte signer public key
 * @param channelPda - Channel PDA (part of signed message)
 * @param amount - Cumulative amount (part of signed message)
 * @param nonce - Ticket nonce (part of signed message)
 */
export function buildEd25519Instruction(params: {
  signature: Uint8Array;
  publicKey: Uint8Array;
  channelPda: PublicKey;
  amount: bigint;
  nonce: bigint;
}): TransactionInstruction {
  const { signature, publicKey, channelPda, amount, nonce } = params;

  // Build the 48-byte message: [channel: 32][amount: 8 LE][nonce: 8 LE]
  const message = Buffer.alloc(48);
  channelPda.toBuffer().copy(message, 0);
  message.writeBigUInt64LE(amount, 32);
  message.writeBigUInt64LE(nonce, 40);

  // Ed25519 instruction format:
  // [num_signatures: 1][padding: 1]
  // [signature_offset: 2][signature_instruction_index: 2]
  // [public_key_offset: 2][public_key_instruction_index: 2]
  // [message_data_offset: 2][message_data_size: 2][message_instruction_index: 2]
  // [signature: 64][public_key: 32][message: 48]

  const numSignatures = 1;
  const padding = 0;
  const signatureOffset = 16; // Header is 16 bytes
  const signatureInstructionIndex = 0xffff; // Same instruction
  const publicKeyOffset = signatureOffset + 64;
  const publicKeyInstructionIndex = 0xffff;
  const messageOffset = publicKeyOffset + 32;
  const messageSize = 48;
  const messageInstructionIndex = 0xffff;

  const instructionData = Buffer.alloc(16 + 64 + 32 + 48);

  // Header
  instructionData.writeUInt8(numSignatures, 0);
  instructionData.writeUInt8(padding, 1);
  instructionData.writeUInt16LE(signatureOffset, 2);
  instructionData.writeUInt16LE(signatureInstructionIndex, 4);
  instructionData.writeUInt16LE(publicKeyOffset, 6);
  instructionData.writeUInt16LE(publicKeyInstructionIndex, 8);
  instructionData.writeUInt16LE(messageOffset, 10);
  instructionData.writeUInt16LE(messageSize, 12);
  instructionData.writeUInt16LE(messageInstructionIndex, 14);

  // Data
  Buffer.from(signature).copy(instructionData, signatureOffset);
  Buffer.from(publicKey).copy(instructionData, publicKeyOffset);
  message.copy(instructionData, messageOffset);

  return new TransactionInstruction({
    keys: [],
    programId: ED25519_PROGRAM_ID,
    data: instructionData,
  });
}

/**
 * Build a settle_channel instruction.
 */
export function buildSettleChannelInstruction(params: {
  worker: PublicKey;
  channelPda: PublicKey;
  mint: PublicKey;
  vaultPda: PublicKey;
  workerTokenAccount: PublicKey;
  amount: bigint;
  nonce: bigint;
  programId?: PublicKey;
}): TransactionInstruction {
  const {
    worker,
    channelPda,
    mint,
    vaultPda,
    workerTokenAccount,
    amount,
    nonce,
    programId = GRAPHENE_PROGRAM_ID,
  } = params;

  return new TransactionInstruction({
    programId,
    keys: [
      { pubkey: worker, isSigner: true, isWritable: false },
      { pubkey: channelPda, isSigner: false, isWritable: true },
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: vaultPda, isSigner: false, isWritable: true },
      { pubkey: workerTokenAccount, isSigner: false, isWritable: true },
      { pubkey: SYSVAR_INSTRUCTIONS_PUBKEY, isSigner: false, isWritable: false },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
    ],
    data: buildSettleChannelData(amount, nonce),
  });
}
