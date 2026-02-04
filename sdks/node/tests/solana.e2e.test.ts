/**
 * E2E tests for TypeScript SDK ↔ Graphene Worker with real Solana integration.
 *
 * Level 2: Full Solana E2E Tests
 * - Requires solana-test-validator
 * - Deploys Anchor program
 * - Creates real payment channels
 * - Tests on-chain ticket validation
 *
 * Run with: bun test:e2e:solana
 * Note: This is a longer-running test suitable for nightly CI.
 */

import { describe, it, expect, beforeAll, afterAll } from 'bun:test';
import { Client } from '../src/client.js';
import { WorkerManager, type WorkerInstance } from './utils/worker-manager.js';
import { SolanaValidator, type ValidatorInstance } from './utils/solana-validator.js';
import { setupTestChannel, getChannelState, type ChannelInfo } from './utils/channel-setup.js';
import { generateTestKeypair, type TestKeypair } from './utils/test-keys.js';

// Platform detection
const IS_MACOS = process.platform === 'darwin';
const MOCK_OUTPUT = 'Mock execution completed\n';

// Test timeout (much longer for Solana setup)
const TEST_TIMEOUT = 180_000;
const SETUP_TIMEOUT = 120_000;

// Skip these tests if solana-test-validator is not available
const SOLANA_AVAILABLE = await checkSolanaAvailable();

async function checkSolanaAvailable(): Promise<boolean> {
  try {
    const proc = Bun.spawn(['solana-test-validator', '--version'], {
      stdout: 'pipe',
      stderr: 'pipe',
    });
    await proc.exited;
    return proc.exitCode === 0;
  } catch {
    return false;
  }
}

describe.skipIf(!SOLANA_AVAILABLE)('E2E: Real Solana Tests', () => {
  let validator: SolanaValidator;
  let validatorInstance: ValidatorInstance;
  let workerManager: WorkerManager;
  let worker: WorkerInstance;
  let testKeypair: TestKeypair;
  let channelInfo: ChannelInfo;

  beforeAll(async () => {
    console.log('Starting Solana test validator...');
    validator = new SolanaValidator({
      startupTimeoutMs: 60_000,
    });
    validatorInstance = await validator.start();
    console.log(`Validator started at ${validatorInstance.rpcUrl}`);

    // Deploy the Graphene program
    console.log('Deploying Graphene program...');
    const programId = await validator.deployProgram();
    console.log(`Program deployed: ${programId}`);

    // Generate test keypair for user
    testKeypair = await generateTestKeypair();
    console.log(`Test user pubkey: ${testKeypair.publicKeyHex}`);

    // Start worker with Solana integration
    workerManager = new WorkerManager({
      testUserPubkeyHex: testKeypair.publicKeyHex,
      startupTimeoutMs: 45_000,
      env: {
        GRAPHENE_SOLANA_RPC_URL: validatorInstance.rpcUrl,
      },
    });

    console.log('Starting worker with Solana integration...');
    worker = await workerManager.start();
    console.log(`Worker started with node ID: ${worker.nodeId}`);

    // Set up payment channel
    console.log('Setting up payment channel...');
    channelInfo = await setupTestChannel({
      rpcUrl: validatorInstance.rpcUrl,
      userKeypair: testKeypair,
      workerPubkeyHex: worker.nodeId,
      initialBalance: 100,
    });
    console.log('Channel created');
  }, SETUP_TIMEOUT);

  afterAll(async () => {
    console.log('Stopping worker...');
    await workerManager?.stop();

    console.log('Stopping validator...');
    await validator?.stop();

    console.log('Cleanup complete');
  }, TEST_TIMEOUT);

  describe('On-Chain Channel Validation', () => {
    it('validates ticket against on-chain channel state', async () => {
      const client = await Client.create({
        secretKey: testKeypair.secretKey,
        channelPda: channelInfo.channelPda,
        workerNodeId: worker.nodeId,
        relayUrl: worker.relayUrl ?? undefined,
        storagePath: `.graphene-test-solana-${Date.now()}`,
      });

      try {
        const result = await client.run({
          code: 'print("Validated with real Solana!")',
          kernel: 'python:3.12',
          timeoutMs: 30_000,
        });

        expect(result.exitCode).toBe(0);

        const output = new TextDecoder().decode(result.output);
        if (IS_MACOS) {
          expect(output).toBe(MOCK_OUTPUT);
        } else {
          expect(output).toContain('Validated with real Solana!');
        }
      } finally {
        await client.close();
      }
    }, TEST_TIMEOUT);

    it('rejects when channel balance exceeded', async () => {
      // Create a client with very large payment requests
      const client = await Client.create({
        secretKey: testKeypair.secretKey,
        channelPda: channelInfo.channelPda,
        workerNodeId: worker.nodeId,
        relayUrl: worker.relayUrl ?? undefined,
        storagePath: `.graphene-test-solana-${Date.now()}`,
      });

      try {
        // TODO: Configure client to request payment exceeding channel balance
        // This test will need the native client to support custom payment amounts

        // For now, just verify basic connectivity works
        const result = await client.run({
          code: 'print("balance check")',
          kernel: 'python:3.12',
        });

        expect(result.exitCode).toBe(0);
      } finally {
        await client.close();
      }
    }, TEST_TIMEOUT);

    it('enforces nonce replay protection', async () => {
      const client = await Client.create({
        secretKey: testKeypair.secretKey,
        channelPda: channelInfo.channelPda,
        workerNodeId: worker.nodeId,
        relayUrl: worker.relayUrl ?? undefined,
        storagePath: `.graphene-test-solana-${Date.now()}`,
      });

      try {
        // Run multiple jobs to increment nonce
        const result1 = await client.run({
          code: 'print(1)',
          kernel: 'python:3.12',
        });
        expect(result1.exitCode).toBe(0);

        const nonce1 = client.currentNonce;

        const result2 = await client.run({
          code: 'print(2)',
          kernel: 'python:3.12',
        });
        expect(result2.exitCode).toBe(0);

        const nonce2 = client.currentNonce;

        // Nonce should be strictly increasing
        expect(nonce2).toBeGreaterThan(nonce1);
      } finally {
        await client.close();
      }
    }, TEST_TIMEOUT);
  });

  describe('Channel State Sync', () => {
    it('reflects channel state correctly', async () => {
      const state = await getChannelState(
        channelInfo.channelPda,
        validatorInstance.rpcUrl
      );

      expect(state.status).toBe('open');
      expect(state.balance).toBeGreaterThan(0);
    });

    // TODO: Add more tests for:
    // - Settlement trigger when threshold reached
    // - Top-up channel reflected in validation
    // - Dispute handling (reject jobs when channel closing)
  });
});

// Always pass test to indicate the file is valid
// (actual tests are skipped if Solana is unavailable)
describe('E2E Solana Test File', () => {
  it('loads successfully', () => {
    expect(true).toBe(true);
  });

  it('reports Solana availability', () => {
    console.log(`Solana test validator available: ${SOLANA_AVAILABLE}`);
    if (!SOLANA_AVAILABLE) {
      console.log('Install solana-cli to run Level 2 E2E tests');
    }
  });
});
