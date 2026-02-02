use anchor_lang::prelude::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChannelStatus {
    #[default]
    Closed,
    Open,
    Disputing,
    Settled,
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
    /// Current balance in channel (lamports or token smallest unit)
    pub balance: u64,
    /// Last settled nonce (monotonically increasing)
    pub nonce: u64,
    /// Current channel status
    pub status: ChannelStatus,
    /// Unix timestamp when dispute window ends (0 if not disputing)
    pub dispute_deadline: i64,
    /// Unix timestamp of last settlement
    pub last_settlement: i64,
    /// PDA bump seed
    pub bump: u8,
}

impl PaymentChannel {
    pub const LEN: usize = 8 // discriminator
        + 32 // user
        + 32 // worker
        + 8  // balance
        + 8  // nonce
        + 1  // status
        + 8  // dispute_deadline
        + 8  // last_settlement
        + 1; // bump
}
