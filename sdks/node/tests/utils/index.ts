/**
 * E2E test utilities.
 */

export { WorkerManager, type WorkerConfig, type WorkerInstance } from './worker-manager.js';
export { generateTestKeypair, keypairFromSeed, testChannelPda, type TestKeypair } from './test-keys.js';
export { SolanaValidator, type ValidatorConfig, type ValidatorInstance } from './solana-validator.js';
export {
  setupTestChannel,
  topUpChannel,
  closeChannel,
  getChannelState,
  getChannelStateCompat,
  testKeypairToSolana,
  hexToPublicKey,
  type ChannelSetupConfig,
  type ChannelInfo,
} from './channel-setup.js';
export {
  GRAPHENE_PROGRAM_ID,
  ED25519_PROGRAM_ID,
  parsePaymentChannel,
  deriveChannelPda,
  deriveVaultPda,
  buildOpenChannelInstruction,
  buildTopUpChannelInstruction,
  buildInitiateCloseInstruction,
  buildEd25519Instruction,
  buildSettleChannelInstruction,
  ChannelState,
  type ParsedPaymentChannel,
} from './solana-types.js';
