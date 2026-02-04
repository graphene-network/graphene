/**
 * Channel setup utilities for Level 2 E2E tests.
 *
 * Creates test wallets, tokens, and payment channels on the local Solana validator.
 */

import {
  Connection,
  Keypair,
  LAMPORTS_PER_SOL,
  PublicKey,
  Transaction,
  sendAndConfirmTransaction,
} from '@solana/web3.js';
import { createMint, createAccount, mintTo, TOKEN_PROGRAM_ID } from '@solana/spl-token';
import type { TestKeypair } from './test-keys.js';
import {
  GRAPHENE_PROGRAM_ID,
  deriveChannelPda,
  deriveVaultPda,
  buildOpenChannelInstruction,
  buildTopUpChannelInstruction,
  buildInitiateCloseInstruction,
  parsePaymentChannel,
  ChannelState,
} from './solana-types.js';

export interface ChannelSetupConfig {
  /** Solana RPC URL */
  rpcUrl: string;
  /** User's Ed25519 keypair */
  userKeypair: TestKeypair;
  /** Worker's Ed25519 public key (hex) */
  workerPubkeyHex: string;
  /** Initial channel balance in token base units (default: 100_000_000 = 100 tokens with 6 decimals) */
  initialBalance?: number;
}

export interface ChannelInfo {
  /** Channel PDA address (32 bytes) */
  channelPda: Uint8Array;
  /** Channel PDA as base58 string */
  channelPdaBase58: string;
  /** User's Solana wallet address */
  userWallet: string;
  /** Token mint address */
  mint: string;
  /** User's token account address */
  userTokenAccount: string;
  /** Initial balance in token base units */
  balance: number;
  /** Worker public key (hex) */
  workerPubkeyHex: string;
}

/**
 * Convert a TestKeypair (noble/ed25519) to a Solana Keypair.
 *
 * The TestKeypair has a 32-byte seed (secretKey) that we use with Keypair.fromSeed().
 * Note: Solana's Keypair.fromSecretKey expects 64 bytes (seed + public key),
 * while Keypair.fromSeed expects just the 32-byte seed.
 */
export function testKeypairToSolana(testKeypair: TestKeypair): Keypair {
  return Keypair.fromSeed(testKeypair.secretKey);
}

/**
 * Convert a hex public key to a Solana PublicKey.
 */
export function hexToPublicKey(hex: string): PublicKey {
  const bytes = Buffer.from(hex, 'hex');
  return new PublicKey(bytes);
}

/**
 * Wait for a transaction to be confirmed with retries.
 */
async function confirmWithRetry(
  connection: Connection,
  signature: string,
  maxRetries: number = 30
): Promise<void> {
  for (let i = 0; i < maxRetries; i++) {
    const status = await connection.getSignatureStatus(signature);
    if (status.value?.confirmationStatus === 'confirmed' ||
        status.value?.confirmationStatus === 'finalized') {
      return;
    }
    await new Promise(resolve => setTimeout(resolve, 1000));
  }
  throw new Error(`Transaction ${signature} not confirmed after ${maxRetries} retries`);
}

/**
 * Set up a payment channel on Solana for testing.
 *
 * This performs:
 * 1. Create user wallet from keypair + airdrop SOL
 * 2. Create test token mint
 * 3. Create user token account and mint tokens
 * 4. Derive channel and vault PDAs
 * 5. Call open_channel instruction
 *
 * @param config - Channel setup configuration
 * @returns Channel info including PDA
 */
export async function setupTestChannel(config: ChannelSetupConfig): Promise<ChannelInfo> {
  const balance = config.initialBalance ?? 100_000_000; // 100 tokens with 6 decimals

  const connection = new Connection(config.rpcUrl, 'confirmed');

  // 1. Create Solana keypair from test keypair
  const userWallet = testKeypairToSolana(config.userKeypair);
  const workerPubkey = hexToPublicKey(config.workerPubkeyHex);

  console.log(`User wallet: ${userWallet.publicKey.toBase58()}`);
  console.log(`Worker pubkey: ${workerPubkey.toBase58()}`);

  // 2. Airdrop SOL to user for transaction fees
  const airdropSig = await connection.requestAirdrop(
    userWallet.publicKey,
    2 * LAMPORTS_PER_SOL
  );
  await confirmWithRetry(connection, airdropSig);
  console.log('Airdropped 2 SOL to user');

  // 3. Create a mint authority keypair and fund it
  const mintAuthority = Keypair.generate();
  const mintAirdropSig = await connection.requestAirdrop(
    mintAuthority.publicKey,
    LAMPORTS_PER_SOL
  );
  await confirmWithRetry(connection, mintAirdropSig);

  // 4. Create test token mint (6 decimals like USDC)
  const mint = await createMint(
    connection,
    mintAuthority, // payer
    mintAuthority.publicKey, // mint authority
    null, // freeze authority
    6 // decimals
  );
  console.log(`Created mint: ${mint.toBase58()}`);

  // 5. Create user token account
  const userTokenAccount = await createAccount(
    connection,
    userWallet, // payer
    mint,
    userWallet.publicKey
  );
  console.log(`Created user token account: ${userTokenAccount.toBase58()}`);

  // 6. Mint tokens to user (enough for initial balance + extra for top-ups)
  const mintAmount = BigInt(balance) * 10n; // 10x the initial balance
  await mintTo(
    connection,
    mintAuthority, // payer
    mint,
    userTokenAccount,
    mintAuthority, // authority
    mintAmount
  );
  console.log(`Minted ${mintAmount} tokens to user`);

  // 7. Derive channel and vault PDAs
  const [channelPda] = deriveChannelPda(userWallet.publicKey, workerPubkey);
  const [vaultPda] = deriveVaultPda(channelPda);

  console.log(`Channel PDA: ${channelPda.toBase58()}`);
  console.log(`Vault PDA: ${vaultPda.toBase58()}`);

  // 8. Build and send open_channel instruction
  const openChannelIx = buildOpenChannelInstruction({
    user: userWallet.publicKey,
    worker: workerPubkey,
    channelPda,
    mint,
    userTokenAccount,
    vaultPda,
    amount: BigInt(balance),
  });

  const tx = new Transaction().add(openChannelIx);
  const sig = await sendAndConfirmTransaction(connection, tx, [userWallet], {
    commitment: 'confirmed',
  });
  console.log(`Channel opened: ${sig}`);

  return {
    channelPda: channelPda.toBytes(),
    channelPdaBase58: channelPda.toBase58(),
    userWallet: userWallet.publicKey.toBase58(),
    mint: mint.toBase58(),
    userTokenAccount: userTokenAccount.toBase58(),
    balance,
    workerPubkeyHex: config.workerPubkeyHex,
  };
}

/**
 * Top up an existing channel with additional tokens.
 *
 * @param channelPda - Channel PDA address (bytes or base58)
 * @param amount - Additional tokens to deposit (in base units)
 * @param rpcUrl - Solana RPC URL
 * @param userKeypair - User's keypair for signing
 * @param mint - Token mint address
 * @param userTokenAccount - User's token account address
 */
export async function topUpChannel(
  channelPda: Uint8Array | string,
  amount: number,
  rpcUrl: string,
  userKeypair: TestKeypair,
  mint: string,
  userTokenAccount: string
): Promise<void> {
  const connection = new Connection(rpcUrl, 'confirmed');
  const userWallet = testKeypairToSolana(userKeypair);

  const channelPdaPubkey = typeof channelPda === 'string'
    ? new PublicKey(channelPda)
    : new PublicKey(channelPda);

  const [vaultPda] = deriveVaultPda(channelPdaPubkey);

  const topUpIx = buildTopUpChannelInstruction({
    user: userWallet.publicKey,
    channelPda: channelPdaPubkey,
    mint: new PublicKey(mint),
    userTokenAccount: new PublicKey(userTokenAccount),
    vaultPda,
    amount: BigInt(amount),
  });

  const tx = new Transaction().add(topUpIx);
  const sig = await sendAndConfirmTransaction(connection, tx, [userWallet], {
    commitment: 'confirmed',
  });

  console.log(`Channel topped up with ${amount} tokens: ${sig}`);
}

/**
 * Initiate channel close (starts the dispute window).
 *
 * @param channelPda - Channel PDA address (bytes or base58)
 * @param rpcUrl - Solana RPC URL
 * @param userKeypair - User's keypair for signing
 */
export async function closeChannel(
  channelPda: Uint8Array | string,
  rpcUrl: string,
  userKeypair: TestKeypair
): Promise<void> {
  const connection = new Connection(rpcUrl, 'confirmed');
  const userWallet = testKeypairToSolana(userKeypair);

  const channelPdaPubkey = typeof channelPda === 'string'
    ? new PublicKey(channelPda)
    : new PublicKey(channelPda);

  const closeIx = buildInitiateCloseInstruction({
    user: userWallet.publicKey,
    channelPda: channelPdaPubkey,
  });

  const tx = new Transaction().add(closeIx);
  const sig = await sendAndConfirmTransaction(connection, tx, [userWallet], {
    commitment: 'confirmed',
  });

  console.log(`Channel close initiated: ${sig}`);
}

/**
 * Get the current on-chain state of a channel.
 *
 * @param channelPda - Channel PDA address (bytes or base58)
 * @param rpcUrl - Solana RPC URL
 * @returns Channel state from the blockchain
 */
export async function getChannelState(
  channelPda: Uint8Array | string,
  rpcUrl: string
): Promise<{
  balance: number;
  spent: number;
  lastNonce: number;
  status: 'open' | 'closing' | 'closed';
  user: string;
  worker: string;
  mint: string;
}> {
  const connection = new Connection(rpcUrl, 'confirmed');

  const channelPdaPubkey = typeof channelPda === 'string'
    ? new PublicKey(channelPda)
    : new PublicKey(channelPda);

  const accountInfo = await connection.getAccountInfo(channelPdaPubkey);

  if (!accountInfo) {
    // Account doesn't exist = channel was closed and account reclaimed
    return {
      balance: 0,
      spent: 0,
      lastNonce: 0,
      status: 'closed',
      user: '',
      worker: '',
      mint: '',
    };
  }

  // Verify it's owned by the Graphene program
  if (!accountInfo.owner.equals(GRAPHENE_PROGRAM_ID)) {
    throw new Error(
      `Channel account not owned by Graphene program. Owner: ${accountInfo.owner.toBase58()}`
    );
  }

  const channel = parsePaymentChannel(Buffer.from(accountInfo.data));

  return {
    balance: Number(channel.balance),
    spent: Number(channel.spent),
    lastNonce: Number(channel.lastNonce),
    status: channel.state === ChannelState.Open ? 'open' : 'closing',
    user: channel.user.toBase58(),
    worker: channel.worker.toBase58(),
    mint: channel.mint.toBase58(),
  };
}

/**
 * Backward-compatible alias for getChannelState that returns the old interface.
 * @deprecated Use getChannelState instead
 */
export async function getChannelStateCompat(
  channelPda: Uint8Array,
  rpcUrl: string
): Promise<{
  balance: number;
  settledAmount: number;
  status: 'open' | 'closing' | 'closed';
}> {
  const state = await getChannelState(channelPda, rpcUrl);
  return {
    balance: state.balance,
    settledAmount: state.spent,
    status: state.status,
  };
}
