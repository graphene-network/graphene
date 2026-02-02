use anchor_lang::prelude::*;

pub mod error;
pub mod instructions;
pub mod state;

use instructions::*;

declare_id!("DHn6uXWDxnBJpkBhBFHiPoDe3S59EnrRQ9qb5rYUdHEs");

#[program]
pub mod graphene {
    use super::*;

    /// One-time program initialization
    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        msg!("Graphene program initialized: {:?}", ctx.program_id);
        Ok(())
    }

    /// Open a payment channel with a worker
    pub fn open_channel(ctx: Context<OpenChannel>, amount: u64) -> Result<()> {
        instructions::channel::open_channel(ctx, amount)
    }

    /// Close a payment channel (initiates 24h dispute window)
    pub fn close_channel(ctx: Context<CloseChannel>) -> Result<()> {
        instructions::channel::close_channel(ctx)
    }

    /// Settle a channel - worker claims with Ed25519 signed ticket
    pub fn settle_channel(ctx: Context<SettleChannel>, amount: u64, nonce: u64) -> Result<()> {
        instructions::channel::settle_channel(ctx, amount, nonce)
    }

    /// Register a new worker with stake
    pub fn register_worker(ctx: Context<RegisterWorker>, stake: u64) -> Result<()> {
        instructions::registry::register_worker(ctx, stake)
    }

    /// Unregister a worker (starts unbonding period)
    pub fn unregister_worker(ctx: Context<UnregisterWorker>) -> Result<()> {
        instructions::registry::unregister_worker(ctx)
    }
}

#[derive(Accounts)]
pub struct Initialize {}
