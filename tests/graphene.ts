import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { Graphene } from "../target/types/graphene";
import { describe, it, expect } from "bun:test";

describe("graphene", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.graphene as Program<Graphene>;

  it("initializes the program", async () => {
    const tx = await program.methods.initialize().rpc();
    console.log("Initialize tx:", tx);
  });

  it("registers a worker", async () => {
    const stake = new anchor.BN(1_000_000); // 0.001 SOL

    const [workerPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("worker"), provider.wallet.publicKey.toBuffer()],
      program.programId
    );

    const tx = await program.methods
      .registerWorker(stake)
      .accounts({
        authority: provider.wallet.publicKey,
        worker: workerPda,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .rpc();

    console.log("Register worker tx:", tx);

    const workerAccount = await program.account.workerRegistry.fetch(workerPda);
    expect(workerAccount.authority.toBase58()).toBe(
      provider.wallet.publicKey.toBase58()
    );
    expect(workerAccount.stake.toNumber()).toBe(stake.toNumber());
    expect(workerAccount.isActive).toBe(true);
  });

  it("unregisters a worker", async () => {
    const [workerPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("worker"), provider.wallet.publicKey.toBuffer()],
      program.programId
    );

    const tx = await program.methods
      .unregisterWorker()
      .accounts({
        authority: provider.wallet.publicKey,
        worker: workerPda,
      })
      .rpc();

    console.log("Unregister worker tx:", tx);

    const workerAccount = await program.account.workerRegistry.fetch(workerPda);
    expect(workerAccount.isActive).toBe(false);
  });
});
