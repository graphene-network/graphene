use anchor_lang::prelude::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChannelState {
    #[default]
    Open,
    Closing,
}

/// Payment channel PDA between a user and worker
/// Seeds: [b"channel", user.key(), worker.key()]
#[account]
#[derive(Default)]
pub struct PaymentChannel {
    /// User who opened the channel
    pub user: Pubkey,
    /// Worker receiving payments
    pub worker: Pubkey,
    /// Token mint for this channel
    pub mint: Pubkey,
    /// Total deposited balance in channel
    pub balance: u64,
    /// Cumulative amount spent (claimed by worker)
    pub spent: u64,
    /// Last settled nonce (monotonically increasing)
    pub last_nonce: u64,
    /// Unix timestamp when dispute window ends (0 if not closing)
    pub timeout: i64,
    /// Current channel state
    pub state: ChannelState,
    /// PDA bump seed
    pub bump: u8,
}

impl PaymentChannel {
    pub const LEN: usize = 8  // discriminator
        + 32 // user
        + 32 // worker
        + 32 // mint
        + 8  // balance
        + 8  // spent
        + 8  // last_nonce
        + 8  // timeout
        + 1  // state
        + 1; // bump
    // Total: 138 bytes
}
