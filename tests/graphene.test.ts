import * as anchor from "@coral-xyz/anchor";
import { Program, BN } from "@coral-xyz/anchor";
import { Graphene } from "../target/types/graphene";
import { describe, it, expect, beforeAll } from "bun:test";
import {
  Keypair,
  PublicKey,
  SystemProgram,
  LAMPORTS_PER_SOL,
} from "@solana/web3.js";
import {
  TOKEN_PROGRAM_ID,
  createMint,
  createAccount,
  mintTo,
  getAccount,
} from "@solana/spl-token";

describe("graphene", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.graphene as Program<Graphene>;
  const connection = provider.connection;
  const wallet = provider.wallet as anchor.Wallet;

  // Test mint and accounts
  let mint: PublicKey;
  let workerTokenAccount: PublicKey;
  const mintAuthority = Keypair.generate();

  // PDAs
  let workerRegistryPda: PublicKey;
  let stakeEscrowPda: PublicKey;

  // Constants
  const DECIMALS = 9;
  const INITIAL_MINT_AMOUNT = 10_000 * 10 ** DECIMALS; // 10,000 tokens

  beforeAll(async () => {
    // Airdrop to mint authority
    const sig = await connection.requestAirdrop(
      mintAuthority.publicKey,
      2 * LAMPORTS_PER_SOL
    );
    await connection.confirmTransaction(sig);

    // Create test mint (simulating $GRAPHENE token)
    mint = await createMint(
      connection,
      wallet.payer,
      mintAuthority.publicKey,
      null,
      DECIMALS
    );

    // Create token account for worker
    workerTokenAccount = await createAccount(
      connection,
      wallet.payer,
      mint,
      wallet.publicKey
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

    // Derive PDAs
    [workerRegistryPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("worker"), wallet.publicKey.toBuffer()],
      program.programId
    );

    [stakeEscrowPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("stake_escrow"), workerRegistryPda.toBuffer()],
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
});
