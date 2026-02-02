import * as anchor from "@coral-xyz/anchor";
import { Program, BN } from "@coral-xyz/anchor";
import { Graphene } from "../target/types/graphene";
import { describe, it, expect, beforeAll } from "bun:test";
import {
  Keypair,
  PublicKey,
  SystemProgram,
  LAMPORTS_PER_SOL,
  Transaction,
  TransactionInstruction,
  SYSVAR_INSTRUCTIONS_PUBKEY,
} from "@solana/web3.js";
import {
  TOKEN_PROGRAM_ID,
  createMint,
  createAccount,
  mintTo,
  getAccount,
} from "@solana/spl-token";
import nacl from "tweetnacl";

// Ed25519 program ID
const ED25519_PROGRAM_ID = new PublicKey(
  "Ed25519SigVerify111111111111111111111111111"
);

/**
 * Create an Ed25519 signature instruction for payment ticket verification
 */
function createEd25519Instruction(
  signerKeypair: Keypair,
  channel: PublicKey,
  amount: bigint,
  nonce: bigint
): TransactionInstruction {
  // Build the 48-byte message: [channel: 32][amount: 8 LE][nonce: 8 LE]
  const message = Buffer.alloc(48);
  channel.toBuffer().copy(message, 0);
  message.writeBigUInt64LE(amount, 32);
  message.writeBigUInt64LE(nonce, 40);

  // Sign the message
  const signature = nacl.sign.detached(message, signerKeypair.secretKey);

  // Build Ed25519 instruction data
  // Format:
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
  signerKeypair.publicKey.toBuffer().copy(instructionData, publicKeyOffset);
  message.copy(instructionData, messageOffset);

  return new TransactionInstruction({
    keys: [],
    programId: ED25519_PROGRAM_ID,
    data: instructionData,
  });
}

describe("graphene", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.graphene as Program<Graphene>;
  const connection = provider.connection;
  const wallet = provider.wallet as anchor.Wallet;

  // Test mint and accounts
  let mint: PublicKey;
  let workerTokenAccount: PublicKey;
  let userTokenAccount: PublicKey;
  const mintAuthority = Keypair.generate();

  // Worker for payment channel tests
  const channelWorker = Keypair.generate();
  let channelWorkerTokenAccount: PublicKey;

  // PDAs for worker registry
  let workerRegistryPda: PublicKey;
  let stakeEscrowPda: PublicKey;

  // PDAs for payment channel
  let channelPda: PublicKey;
  let channelBump: number;
  let vaultPda: PublicKey;
  let vaultBump: number;

  // Constants
  const DECIMALS = 9;
  const INITIAL_MINT_AMOUNT = 10_000 * 10 ** DECIMALS; // 10,000 tokens
  const CHANNEL_DEPOSIT = 100_000_000n; // 100 tokens (with 6 decimals for channel tests)

  beforeAll(async () => {
    // Airdrop to mint authority
    const sig = await connection.requestAirdrop(
      mintAuthority.publicKey,
      2 * LAMPORTS_PER_SOL
    );
    await connection.confirmTransaction(sig);

    // Airdrop SOL to channel worker for fees
    const sig2 = await connection.requestAirdrop(
      channelWorker.publicKey,
      2 * LAMPORTS_PER_SOL
    );
    await connection.confirmTransaction(sig2);

    // Create test mint (simulating $GRAPHENE token)
    mint = await createMint(
      connection,
      wallet.payer,
      mintAuthority.publicKey,
      null,
      DECIMALS
    );

    // Create token account for worker registry tests
    workerTokenAccount = await createAccount(
      connection,
      wallet.payer,
      mint,
      wallet.publicKey
    );

    // Create user token account (for payment channel tests)
    userTokenAccount = await createAccount(
      connection,
      wallet.payer,
      mint,
      wallet.publicKey
    );

    // Create channel worker token account
    channelWorkerTokenAccount = await createAccount(
      connection,
      wallet.payer,
      mint,
      channelWorker.publicKey
    );

    // Mint tokens to worker
    await mintTo(
      connection,
      wallet.payer,
      mint,
      workerTokenAccount,
      mintAuthority,
      INITIAL_MINT_AMOUNT
    );

    // Mint tokens to user token account for channel tests
    await mintTo(
      connection,
      wallet.payer,
      mint,
      userTokenAccount,
      mintAuthority,
      INITIAL_MINT_AMOUNT
    );

    // Derive PDAs for worker registry
    [workerRegistryPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("worker"), wallet.publicKey.toBuffer()],
      program.programId
    );

    [stakeEscrowPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("stake_escrow"), workerRegistryPda.toBuffer()],
      program.programId
    );

    // Derive PDAs for payment channel
    [channelPda, channelBump] = PublicKey.findProgramAddressSync(
      [
        Buffer.from("channel"),
        wallet.publicKey.toBuffer(),
        channelWorker.publicKey.toBuffer(),
      ],
      program.programId
    );

    [vaultPda, vaultBump] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault"), channelPda.toBuffer()],
      program.programId
    );
  });

  it("initializes the program", async () => {
    const tx = await program.methods.initialize().rpc();
    console.log("Initialize tx:", tx);
  });

  describe("worker registration", () => {
    it("registers a worker with SPL token stake", async () => {
      const stakeAmount = new BN(1000 * 10 ** DECIMALS); // 1000 tokens
      const capabilities = { maxVcpu: 8, maxMemoryMb: 32768 }; // 8 vCPU, 32GB RAM

      // Min stake = 100 + (50*8) + (10*32) = 100 + 400 + 320 = 820
      // We're staking 1000, which is > 820

      const tx = await program.methods
        .registerWorker(stakeAmount, capabilities)
        .accounts({
          authority: wallet.publicKey,
          workerRegistry: workerRegistryPda,
          mint: mint,
          authorityTokenAccount: workerTokenAccount,
          stakeEscrow: stakeEscrowPda,
          tokenProgram: TOKEN_PROGRAM_ID,
          associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .rpc();

      console.log("Register worker tx:", tx);

      // Verify worker registry state
      const workerAccount =
        await program.account.workerRegistry.fetch(workerRegistryPda);
      expect(workerAccount.authority.toBase58()).toBe(
        wallet.publicKey.toBase58()
      );
      expect(workerAccount.stakeAmount.toNumber()).toBe(stakeAmount.toNumber());
      expect(workerAccount.stakeMint.toBase58()).toBe(mint.toBase58());
      expect(workerAccount.state).toEqual({ active: {} });
      expect(workerAccount.capabilities.maxVcpu).toBe(8);
      expect(workerAccount.capabilities.maxMemoryMb).toBe(32768);

      // Verify escrow received tokens
      const escrowAccount = await getAccount(connection, stakeEscrowPda);
      expect(Number(escrowAccount.amount)).toBe(stakeAmount.toNumber());
    });

    it("rejects registration with insufficient stake", async () => {
      // Create a second worker keypair
      const worker2 = Keypair.generate();

      // Airdrop to second worker
      const sig = await connection.requestAirdrop(
        worker2.publicKey,
        2 * LAMPORTS_PER_SOL
      );
      await connection.confirmTransaction(sig);

      // Create token account for worker2
      const worker2TokenAccount = await createAccount(
        connection,
        wallet.payer,
        mint,
        worker2.publicKey
      );

      // Mint some tokens (but not enough)
      await mintTo(
        connection,
        wallet.payer,
        mint,
        worker2TokenAccount,
        mintAuthority,
        100 * 10 ** DECIMALS // Only 100 tokens
      );

      const [worker2RegistryPda] = PublicKey.findProgramAddressSync(
        [Buffer.from("worker"), worker2.publicKey.toBuffer()],
        program.programId
      );

      const [worker2EscrowPda] = PublicKey.findProgramAddressSync(
        [Buffer.from("stake_escrow"), worker2RegistryPda.toBuffer()],
        program.programId
      );

      // Min stake for 8 vCPU, 32GB = 100 + 400 + 320 = 820 smallest units
      // Stake 500 smallest units (less than 820)
      const stakeAmount = new BN(500);
      const capabilities = { maxVcpu: 8, maxMemoryMb: 32768 };

      try {
        await program.methods
          .registerWorker(stakeAmount, capabilities)
          .accounts({
            authority: worker2.publicKey,
            workerRegistry: worker2RegistryPda,
            mint: mint,
            authorityTokenAccount: worker2TokenAccount,
            stakeEscrow: worker2EscrowPda,
            tokenProgram: TOKEN_PROGRAM_ID,
            associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
            systemProgram: SystemProgram.programId,
          })
          .signers([worker2])
          .rpc();
        throw new Error("Expected InsufficientStake error");
      } catch (err: unknown) {
        expect(String(err)).toContain("InsufficientStake");
      }
    });
  });

  describe("unbonding", () => {
    it("initiates unbonding", async () => {
      const tx = await program.methods
        .initiateUnbonding()
        .accounts({
          authority: wallet.publicKey,
          workerRegistry: workerRegistryPda,
        })
        .rpc();

      console.log("Initiate unbonding tx:", tx);

      const workerAccount =
        await program.account.workerRegistry.fetch(workerRegistryPda);
      expect(workerAccount.state).toEqual({ unbonding: {} });
      expect(workerAccount.unbondingStart.toNumber()).toBeGreaterThan(0);
    });

    it("rejects early unbonding completion", async () => {
      try {
        await program.methods
          .completeUnbonding()
          .accounts({
            authority: wallet.publicKey,
            workerRegistry: workerRegistryPda,
            mint: mint,
            authorityTokenAccount: workerTokenAccount,
            stakeEscrow: stakeEscrowPda,
            tokenProgram: TOKEN_PROGRAM_ID,
          })
          .rpc();
        throw new Error("Expected UnbondingNotComplete error");
      } catch (err: unknown) {
        expect(String(err)).toContain("UnbondingNotComplete");
      }
    });

    it("rejects double unbonding initiation", async () => {
      try {
        await program.methods
          .initiateUnbonding()
          .accounts({
            authority: wallet.publicKey,
            workerRegistry: workerRegistryPda,
          })
          .rpc();
        throw new Error("Expected WorkerNotActive error");
      } catch (err: unknown) {
        expect(String(err)).toContain("WorkerNotActive");
      }
    });
  });

  describe("edge cases", () => {
    it("prevents double registration", async () => {
      // First registration already happened, try again
      const stakeAmount = new BN(1000 * 10 ** DECIMALS);
      const capabilities = { maxVcpu: 4, maxMemoryMb: 16384 };

      try {
        await program.methods
          .registerWorker(stakeAmount, capabilities)
          .accounts({
            authority: wallet.publicKey,
            workerRegistry: workerRegistryPda,
            mint: mint,
            authorityTokenAccount: workerTokenAccount,
            stakeEscrow: stakeEscrowPda,
            tokenProgram: TOKEN_PROGRAM_ID,
            associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
            systemProgram: SystemProgram.programId,
          })
          .rpc();
        throw new Error("Expected account already in use error");
      } catch (err: unknown) {
        // This will fail because the account already exists
        expect(err).toBeDefined();
      }
    });
  });

  describe("Payment Channel", () => {
    it("opens a payment channel", async () => {
      const amount = new BN(Number(CHANNEL_DEPOSIT));

      const tx = await program.methods
        .openChannel(amount)
        .accounts({
          user: wallet.publicKey,
          worker: channelWorker.publicKey,
          channel: channelPda,
          mint: mint,
          userTokenAccount: userTokenAccount,
          vault: vaultPda,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .rpc();

      console.log("Open channel tx:", tx);

      // Verify channel state
      const channelAccount = await program.account.paymentChannel.fetch(
        channelPda
      );
      expect(channelAccount.user.toBase58()).toBe(
        wallet.publicKey.toBase58()
      );
      expect(channelAccount.worker.toBase58()).toBe(channelWorker.publicKey.toBase58());
      expect(channelAccount.mint.toBase58()).toBe(mint.toBase58());
      expect(channelAccount.balance.toNumber()).toBe(Number(CHANNEL_DEPOSIT));
      expect(channelAccount.spent.toNumber()).toBe(0);
      expect(channelAccount.lastNonce.toNumber()).toBe(0);
      expect(channelAccount.state).toEqual({ open: {} });

      // Verify vault balance
      const vaultAccount = await getAccount(connection, vaultPda);
      expect(vaultAccount.amount).toBe(CHANNEL_DEPOSIT);
    });

    it("tops up a payment channel", async () => {
      const topUpAmount = new BN(50_000_000); // 50 tokens

      const tx = await program.methods
        .topUpChannel(topUpAmount)
        .accounts({
          user: wallet.publicKey,
          channel: channelPda,
          mint: mint,
          userTokenAccount: userTokenAccount,
          vault: vaultPda,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .rpc();

      console.log("Top up channel tx:", tx);

      // Verify channel state
      const channelAccount = await program.account.paymentChannel.fetch(
        channelPda
      );
      expect(channelAccount.balance.toNumber()).toBe(150_000_000); // 100 + 50

      // Verify vault balance
      const vaultAccount = await getAccount(connection, vaultPda);
      expect(vaultAccount.amount).toBe(150_000_000n);
    });

    it("settles a payment with Ed25519 ticket", async () => {
      const amount = 10_000_000n; // 10 tokens
      const nonce = 1n;

      // Create Ed25519 verification instruction signed by the user
      const ed25519Ix = createEd25519Instruction(
        wallet.payer,
        channelPda,
        amount,
        nonce
      );

      // Build settle instruction
      const settleIx = await program.methods
        .settleChannel(new BN(amount.toString()), new BN(nonce.toString()))
        .accounts({
          worker: channelWorker.publicKey,
          channel: channelPda,
          mint: mint,
          vault: vaultPda,
          workerTokenAccount: channelWorkerTokenAccount,
          ixSysvar: SYSVAR_INSTRUCTIONS_PUBKEY,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .instruction();

      // Send transaction with both instructions
      const tx = new Transaction().add(ed25519Ix).add(settleIx);
      const sig = await provider.sendAndConfirm(tx, [channelWorker]);
      console.log("Settle channel tx:", sig);

      // Verify channel state
      const channelAccount = await program.account.paymentChannel.fetch(
        channelPda
      );
      expect(channelAccount.spent.toNumber()).toBe(10_000_000);
      expect(channelAccount.lastNonce.toNumber()).toBe(1);

      // Verify worker received tokens
      const workerAccount = await getAccount(
        connection,
        channelWorkerTokenAccount
      );
      expect(workerAccount.amount).toBe(amount);
    });

    it("fails to settle with invalid nonce", async () => {
      const amount = 5_000_000n;
      const nonce = 1n; // Same nonce as before - should fail

      const ed25519Ix = createEd25519Instruction(
        wallet.payer,
        channelPda,
        amount,
        nonce
      );

      const settleIx = await program.methods
        .settleChannel(new BN(amount.toString()), new BN(nonce.toString()))
        .accounts({
          worker: channelWorker.publicKey,
          channel: channelPda,
          mint: mint,
          vault: vaultPda,
          workerTokenAccount: channelWorkerTokenAccount,
          ixSysvar: SYSVAR_INSTRUCTIONS_PUBKEY,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .instruction();

      const tx = new Transaction().add(ed25519Ix).add(settleIx);

      try {
        await provider.sendAndConfirm(tx, [channelWorker]);
        expect(true).toBe(false); // Should not reach here
      } catch (e: any) {
        expect(e.toString()).toContain("InvalidNonce");
      }
    });

    it("fails to settle with wrong signer", async () => {
      const amount = 5_000_000n;
      const nonce = 2n;

      // Sign with wrong key (worker instead of user)
      const ed25519Ix = createEd25519Instruction(channelWorker, channelPda, amount, nonce);

      const settleIx = await program.methods
        .settleChannel(new BN(amount.toString()), new BN(nonce.toString()))
        .accounts({
          worker: channelWorker.publicKey,
          channel: channelPda,
          mint: mint,
          vault: vaultPda,
          workerTokenAccount: channelWorkerTokenAccount,
          ixSysvar: SYSVAR_INSTRUCTIONS_PUBKEY,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .instruction();

      const tx = new Transaction().add(ed25519Ix).add(settleIx);

      try {
        await provider.sendAndConfirm(tx, [channelWorker]);
        expect(true).toBe(false); // Should not reach here
      } catch (e: any) {
        expect(e.toString()).toContain("SignatureVerificationFailed");
      }
    });

    it("fails to overdraft channel", async () => {
      const amount = 200_000_000n; // More than balance
      const nonce = 2n;

      const ed25519Ix = createEd25519Instruction(
        wallet.payer,
        channelPda,
        amount,
        nonce
      );

      const settleIx = await program.methods
        .settleChannel(new BN(amount.toString()), new BN(nonce.toString()))
        .accounts({
          worker: channelWorker.publicKey,
          channel: channelPda,
          mint: mint,
          vault: vaultPda,
          workerTokenAccount: channelWorkerTokenAccount,
          ixSysvar: SYSVAR_INSTRUCTIONS_PUBKEY,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .instruction();

      const tx = new Transaction().add(ed25519Ix).add(settleIx);

      try {
        await provider.sendAndConfirm(tx, [channelWorker]);
        expect(true).toBe(false); // Should not reach here
      } catch (e: any) {
        expect(e.toString()).toContain("InsufficientBalance");
      }
    });
  });

  describe("Channel Close Flow", () => {
    // Create a separate channel for close testing
    let closeChannelPda: PublicKey;
    let closeVaultPda: PublicKey;
    const closeWorker = Keypair.generate();
    let closeWorkerTokenAccount: PublicKey;

    beforeAll(async () => {
      // Airdrop SOL to close worker
      const sig = await connection.requestAirdrop(
        closeWorker.publicKey,
        2 * LAMPORTS_PER_SOL
      );
      await connection.confirmTransaction(sig);

      // Create worker token account
      closeWorkerTokenAccount = await createAccount(
        connection,
        wallet.payer,
        mint,
        closeWorker.publicKey
      );

      // Derive PDAs
      [closeChannelPda] = PublicKey.findProgramAddressSync(
        [
          Buffer.from("channel"),
          wallet.publicKey.toBuffer(),
          closeWorker.publicKey.toBuffer(),
        ],
        program.programId
      );

      [closeVaultPda] = PublicKey.findProgramAddressSync(
        [Buffer.from("vault"), closeChannelPda.toBuffer()],
        program.programId
      );

      // Open a channel for close testing
      const amount = new BN(50_000_000); // 50 tokens
      await program.methods
        .openChannel(amount)
        .accounts({
          user: wallet.publicKey,
          worker: closeWorker.publicKey,
          channel: closeChannelPda,
          mint: mint,
          userTokenAccount: userTokenAccount,
          vault: closeVaultPda,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .rpc();
    });

    it("initiates channel close", async () => {
      const tx = await program.methods
        .initiateClose()
        .accounts({
          user: wallet.publicKey,
          channel: closeChannelPda,
        })
        .rpc();

      console.log("Initiate close tx:", tx);

      const channelAccount = await program.account.paymentChannel.fetch(
        closeChannelPda
      );
      expect(channelAccount.state).toEqual({ closing: {} });
      expect(channelAccount.timeout.toNumber()).toBeGreaterThan(0);
    });

    it("fails to force close before timeout", async () => {
      try {
        await program.methods
          .forceClose()
          .accounts({
            user: wallet.publicKey,
            channel: closeChannelPda,
            mint: mint,
            vault: closeVaultPda,
            userTokenAccount: userTokenAccount,
            tokenProgram: TOKEN_PROGRAM_ID,
          })
          .rpc();

        expect(true).toBe(false); // Should not reach here
      } catch (e: any) {
        expect(e.toString()).toContain("TimeoutNotExpired");
      }
    });

    it("worker can still settle during closing period", async () => {
      const amount = 5_000_000n;
      const nonce = 1n;

      const ed25519Ix = createEd25519Instruction(
        wallet.payer,
        closeChannelPda,
        amount,
        nonce
      );

      const settleIx = await program.methods
        .settleChannel(new BN(amount.toString()), new BN(nonce.toString()))
        .accounts({
          worker: closeWorker.publicKey,
          channel: closeChannelPda,
          mint: mint,
          vault: closeVaultPda,
          workerTokenAccount: closeWorkerTokenAccount,
          ixSysvar: SYSVAR_INSTRUCTIONS_PUBKEY,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .instruction();

      const tx = new Transaction().add(ed25519Ix).add(settleIx);
      const sig = await provider.sendAndConfirm(tx, [closeWorker]);
      console.log("Settle during close tx:", sig);

      const channelAccount = await program.account.paymentChannel.fetch(
        closeChannelPda
      );
      expect(channelAccount.spent.toNumber()).toBe(5_000_000);
    });
  });

  describe("Cooperative Close", () => {
    let coopChannelPda: PublicKey;
    let coopVaultPda: PublicKey;
    const coopWorker = Keypair.generate();
    let coopWorkerTokenAccount: PublicKey;

    beforeAll(async () => {
      // Airdrop SOL to coop worker
      const sig = await connection.requestAirdrop(
        coopWorker.publicKey,
        2 * LAMPORTS_PER_SOL
      );
      await connection.confirmTransaction(sig);

      // Create worker token account
      coopWorkerTokenAccount = await createAccount(
        connection,
        wallet.payer,
        mint,
        coopWorker.publicKey
      );

      // Derive PDAs
      [coopChannelPda] = PublicKey.findProgramAddressSync(
        [
          Buffer.from("channel"),
          wallet.publicKey.toBuffer(),
          coopWorker.publicKey.toBuffer(),
        ],
        program.programId
      );

      [coopVaultPda] = PublicKey.findProgramAddressSync(
        [Buffer.from("vault"), coopChannelPda.toBuffer()],
        program.programId
      );

      // Open a channel for cooperative close testing
      const amount = new BN(100_000_000); // 100 tokens
      await program.methods
        .openChannel(amount)
        .accounts({
          user: wallet.publicKey,
          worker: coopWorker.publicKey,
          channel: coopChannelPda,
          mint: mint,
          userTokenAccount: userTokenAccount,
          vault: coopVaultPda,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .rpc();
    });

    it("cooperatively closes channel", async () => {
      const finalSpent = new BN(30_000_000); // Worker gets 30, user gets 70

      const userBalanceBefore = (
        await getAccount(connection, userTokenAccount)
      ).amount;

      const tx = await program.methods
        .cooperativeClose(finalSpent)
        .accounts({
          user: wallet.publicKey,
          worker: coopWorker.publicKey,
          channel: coopChannelPda,
          mint: mint,
          vault: coopVaultPda,
          userTokenAccount: userTokenAccount,
          workerTokenAccount: coopWorkerTokenAccount,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .signers([coopWorker])
        .rpc();

      console.log("Cooperative close tx:", tx);

      // Verify worker received their portion
      const workerAccount = await getAccount(
        connection,
        coopWorkerTokenAccount
      );
      expect(workerAccount.amount).toBe(30_000_000n);

      // Verify user received refund
      const userBalanceAfter = (
        await getAccount(connection, userTokenAccount)
      ).amount;
      expect(userBalanceAfter - userBalanceBefore).toBe(70_000_000n);

      // Verify channel is closed
      try {
        await program.account.paymentChannel.fetch(coopChannelPda);
        expect(true).toBe(false); // Should not reach here
      } catch (e: any) {
        expect(e.toString()).toContain("Account does not exist");
      }
    });
  });
});
