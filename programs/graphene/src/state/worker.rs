use anchor_lang::prelude::*;

/// Worker state machine
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkerState {
    #[default]
    Active,
    Unbonding,
    Slashed,
}

/// Worker capabilities determining minimum stake requirements
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default)]
pub struct WorkerCapabilities {
    /// Maximum vCPUs this worker can provide
    pub max_vcpu: u8,
    /// Maximum memory in MB this worker can provide
    pub max_memory_mb: u32,
}

/// Worker registry PDA
/// Seeds: [b"worker", authority.key()]
#[account]
#[derive(Default)]
pub struct WorkerRegistry {
    /// Authority that controls this worker
    pub authority: Pubkey,
    /// Staked $GRAPHENE token amount
    pub stake_amount: u64,
    /// $GRAPHENE token mint address
    pub stake_mint: Pubkey,
    /// Unix timestamp when worker was registered
    pub registered_at: i64,
    /// Current worker state
    pub state: WorkerState,
    /// Unix timestamp when unbonding started (0 if not unbonding)
    pub unbonding_start: i64,
    /// Worker capabilities (determines min stake)
    pub capabilities: WorkerCapabilities,
    /// PDA bump seed
    pub bump: u8,
}

impl WorkerRegistry {
    pub const LEN: usize = 8  // discriminator
        + 32 // authority
        + 8  // stake_amount
        + 32 // stake_mint
        + 8  // registered_at
        + 1  // state
        + 8  // unbonding_start
        + 1  // capabilities.max_vcpu
        + 4  // capabilities.max_memory_mb
        + 1; // bump
}

/// Calculate minimum stake required for given capabilities
/// Formula: base + (50 * vcpu) + (10 * memory_gb)
pub fn calculate_min_stake(capabilities: &WorkerCapabilities) -> u64 {
    let base: u64 = 100;
    let per_vcpu: u64 = 50 * capabilities.max_vcpu as u64;
    let per_gb_ram: u64 = 10 * (capabilities.max_memory_mb as u64 / 1024);
    base + per_vcpu + per_gb_ram
}

/// 14 days in seconds
pub const UNBONDING_PERIOD: i64 = 14 * 24 * 60 * 60;
