use anchor_lang::prelude::*;

/// Worker registry PDA
/// Seeds: [b"worker", authority.key()]
#[account]
#[derive(Default)]
pub struct WorkerRegistry {
    /// Authority that controls this worker
    pub authority: Pubkey,
    /// Staked amount (in lamports)
    pub stake: u64,
    /// Whether the worker is currently active
    pub is_active: bool,
    /// Unix timestamp when worker was registered
    pub registered_at: i64,
    /// PDA bump seed
    pub bump: u8,
}

impl WorkerRegistry {
    pub const LEN: usize = 8  // discriminator
        + 32 // authority
        + 8  // stake
        + 1  // is_active
        + 8  // registered_at
        + 1; // bump
}
