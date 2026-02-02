use anchor_lang::prelude::*;

pub mod error;
pub mod instructions;
pub mod state;

use instructions::*;
use state::WorkerCapabilities;

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

    /// Register a new worker with SPL token stake
    pub fn register_worker(
        ctx: Context<RegisterWorker>,
        stake_amount: u64,
        capabilities: WorkerCapabilities,
    ) -> Result<()> {
        instructions::registry::register_worker(ctx, stake_amount, capabilities)
    }

    /// Initiate unbonding - starts 14-day countdown
    pub fn initiate_unbonding(ctx: Context<InitiateUnbonding>) -> Result<()> {
        instructions::registry::initiate_unbonding(ctx)
    }

    /// Complete unbonding after 14-day period - returns stake and closes account
    pub fn complete_unbonding(ctx: Context<CompleteUnbonding>) -> Result<()> {
        instructions::registry::complete_unbonding(ctx)
    }
}

#[derive(Accounts)]
pub struct Initialize {}
