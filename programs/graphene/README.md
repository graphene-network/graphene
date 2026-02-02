# Graphene Solana Program

On-chain settlement layer for the Graphene compute network.

## Build

```bash
anchor build
```

## Test

```bash
anchor test
```

## Manual Verification

### 1. Start localnet and deploy

```bash
solana-test-validator --reset &
anchor deploy
```

### 2. Create test mint and fund worker

```bash
# Create $GRAPHENE mint
spl-token create-token --decimals 9
# Note the mint address, e.g., GRAPHxxxxx

# Create token account for worker
spl-token create-account <MINT_ADDRESS>

# Mint tokens to worker
spl-token mint <MINT_ADDRESS> 1000
```

### 3. Register worker

```typescript
import * as anchor from "@coral-xyz/anchor";
import { PublicKey, SystemProgram } from "@solana/web3.js";
import { TOKEN_PROGRAM_ID } from "@solana/spl-token";

const capabilities = { maxVcpu: 4, maxMemoryMb: 16384 };
const stakeAmount = new anchor.BN(500);

// Derive PDAs
const [workerPda] = PublicKey.findProgramAddressSync(
  [Buffer.from("worker"), wallet.publicKey.toBuffer()],
  program.programId
);
const [escrowPda] = PublicKey.findProgramAddressSync(
  [Buffer.from("stake_escrow"), workerPda.toBuffer()],
  program.programId
);

await program.methods
  .registerWorker(stakeAmount, capabilities)
  .accounts({
    authority: wallet.publicKey,
    workerRegistry: workerPda,
    mint: GRAPHENE_MINT,
    authorityTokenAccount: workerAta,
    stakeEscrow: escrowPda,
    tokenProgram: TOKEN_PROGRAM_ID,
    associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
    systemProgram: SystemProgram.programId,
  })
  .rpc();
```

### 4. Verify registration

```typescript
const worker = await program.account.workerRegistry.fetch(workerPda);
console.log("State:", worker.state); // { active: {} }
console.log("Stake:", worker.stakeAmount.toNumber());
console.log("Capabilities:", worker.capabilities);

// Check escrow balance
const escrowBalance = await connection.getTokenAccountBalance(escrowPda);
console.log("Escrow:", escrowBalance.value.amount);
```

### 5. Initiate unbonding

```typescript
await program.methods
  .initiateUnbonding()
  .accounts({
    authority: wallet.publicKey,
    workerRegistry: workerPda,
  })
  .rpc();

const worker = await program.account.workerRegistry.fetch(workerPda);
console.log("State:", worker.state); // { unbonding: {} }
console.log("Unbonding started:", worker.unbondingStart.toNumber());
```

### 6. Complete unbonding (after 14 days)

```typescript
await program.methods
  .completeUnbonding()
  .accounts({
    authority: wallet.publicKey,
    workerRegistry: workerPda,
    mint: GRAPHENE_MINT,
    authorityTokenAccount: workerAta,
    stakeEscrow: escrowPda,
    tokenProgram: TOKEN_PROGRAM_ID,
  })
  .rpc();

// Worker registry account is now closed
// Stake returned to authority token account
```

For localnet testing, use `solana-test-validator` with `--warp-slot` to skip the 14-day wait.

## Staking Formula

Minimum stake required based on worker capabilities:

```
min_stake = 100 + (50 * max_vcpu) + (10 * max_memory_gb)
```

Example: 8 vCPU, 32GB RAM = 100 + 400 + 320 = 820 tokens
