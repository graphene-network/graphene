/**
 * Channel setup utilities for Level 2 E2E tests.
 *
 * Creates test wallets, USDC tokens, and payment channels
 * on the local Solana validator.
 */

import type { TestKeypair } from './test-keys.js';

export interface ChannelSetupConfig {
  /** Solana RPC URL */
  rpcUrl: string;
  /** User's Ed25519 keypair */
  userKeypair: TestKeypair;
  /** Worker's Ed25519 public key (hex) */
  workerPubkeyHex: string;
  /** Initial channel balance in USDC (default: 100) */
  initialBalance?: number;
}

export interface ChannelInfo {
  /** Channel PDA address (32 bytes) */
  channelPda: Uint8Array;
  /** User's Solana wallet address */
  userWallet: string;
  /** USDC mint address */
  usdcMint: string;
  /** Initial balance in USDC */
  balance: number;
}

/**
 * Set up a payment channel on Solana for testing.
 *
 * This performs:
 * 1. Create user wallet + airdrop SOL
 * 2. Create/mint test USDC tokens
 * 3. Call open_channel instruction
 *
 * @param config - Channel setup configuration
 * @returns Channel info including PDA
 */
export async function setupTestChannel(config: ChannelSetupConfig): Promise<ChannelInfo> {
  const balance = config.initialBalance ?? 100;

  // TODO: Implement actual Solana channel setup
  // This would use @solana/web3.js and @coral-xyz/anchor to:
  //
  // 1. Create Keypair from user's Ed25519 secret key
  // const userWallet = Keypair.fromSecretKey(config.userKeypair.secretKey);
  //
  // 2. Request SOL airdrop
  // const connection = new Connection(config.rpcUrl);
  // await connection.requestAirdrop(userWallet.publicKey, LAMPORTS_PER_SOL * 10);
  //
  // 3. Create test USDC mint and token account
  // const usdcMint = await createMint(...);
  // await mintTo(usdcMint, userTokenAccount, balance * 1_000_000);
  //
  // 4. Initialize Anchor program and call open_channel
  // const program = new Program(idl, programId, provider);
  // const [channelPda] = PublicKey.findProgramAddressSync(
  //   [Buffer.from("channel"), userWallet.publicKey.toBuffer(), workerPubkey.toBuffer()],
  //   program.programId
  // );
  // await program.methods.openChannel(new BN(balance * 1_000_000))
  //   .accounts({ ... })
  //   .signers([userWallet])
  //   .rpc();

  console.log(`Channel setup not yet implemented for RPC: ${config.rpcUrl}`);
  console.log(`Would create channel with ${balance} USDC for user ${config.userKeypair.publicKeyHex.slice(0, 16)}...`);

  // Return placeholder channel info
  // In production, channelPda would be derived from program seeds
  return {
    channelPda: new Uint8Array(32).fill(0x01), // Matches test channel in server.rs
    userWallet: config.userKeypair.publicKeyHex,
    usdcMint: '4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU', // Placeholder devnet USDC
    balance,
  };
}

/**
 * Top up an existing channel with additional USDC.
 *
 * @param channelPda - Channel PDA address
 * @param amount - Additional USDC to deposit
 * @param rpcUrl - Solana RPC URL
 * @param userKeypair - User's keypair for signing
 */
export async function topUpChannel(
  channelPda: Uint8Array,
  amount: number,
  rpcUrl: string,
  userKeypair: TestKeypair
): Promise<void> {
  // TODO: Implement channel top-up
  // await program.methods.topUp(new BN(amount * 1_000_000))
  //   .accounts({ channel: channelPda, ... })
  //   .signers([userWallet])
  //   .rpc();

  console.log(`Channel top-up not yet implemented: ${amount} USDC`);
}

/**
 * Close a channel and retrieve remaining balance.
 *
 * @param channelPda - Channel PDA address
 * @param rpcUrl - Solana RPC URL
 * @param userKeypair - User's keypair for signing
 */
export async function closeChannel(
  channelPda: Uint8Array,
  rpcUrl: string,
  userKeypair: TestKeypair
): Promise<void> {
  // TODO: Implement channel close
  // This initiates the dispute period before final settlement

  console.log('Channel close not yet implemented');
}

/**
 * Get the current on-chain state of a channel.
 *
 * @param channelPda - Channel PDA address
 * @param rpcUrl - Solana RPC URL
 * @returns Channel state from the blockchain
 */
export async function getChannelState(
  channelPda: Uint8Array,
  rpcUrl: string
): Promise<{
  balance: number;
  settledAmount: number;
  status: 'open' | 'closing' | 'closed';
}> {
  // TODO: Implement channel state query
  // const accountInfo = await connection.getAccountInfo(channelPda);
  // const channelData = program.coder.accounts.decode('Channel', accountInfo.data);

  console.log('Channel state query not yet implemented');

  return {
    balance: 100_000_000, // 100 USDC in micros
    settledAmount: 0,
    status: 'open',
  };
}
