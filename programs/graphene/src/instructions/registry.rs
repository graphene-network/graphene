use anchor_lang::prelude::*;
use anchor_lang::system_program::{transfer, Transfer};

use crate::error::GrapheneError;
use crate::state::WorkerRegistry;

/// Register a new worker with stake
#[derive(Accounts)]
pub struct RegisterWorker<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        init,
        payer = authority,
        space = WorkerRegistry::LEN,
        seeds = [b"worker", authority.key().as_ref()],
        bump
    )]
    pub worker: Account<'info, WorkerRegistry>,

    pub system_program: Program<'info, System>,
}

pub fn register_worker(ctx: Context<RegisterWorker>, stake: u64) -> Result<()> {
    let authority_key = ctx.accounts.authority.key();

    // Transfer stake to worker PDA first (before mutable borrow)
    if stake > 0 {
        let cpi_accounts = Transfer {
            from: ctx.accounts.authority.to_account_info(),
            to: ctx.accounts.worker.to_account_info(),
        };
        let cpi_program = ctx.accounts.system_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        transfer(cpi_ctx, stake)?;
    }

    // Now take mutable borrow to update state
    let worker = &mut ctx.accounts.worker;
    worker.authority = authority_key;
    worker.stake = stake;
    worker.is_active = true;
    worker.registered_at = Clock::get()?.unix_timestamp;
    worker.bump = ctx.bumps.worker;

    msg!("Worker registered: authority={}, stake={}", authority_key, stake);

    Ok(())
}

/// Unregister a worker (starts unbonding period)
#[derive(Accounts)]
pub struct UnregisterWorker<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        mut,
        seeds = [b"worker", authority.key().as_ref()],
        bump = worker.bump,
        constraint = worker.authority == authority.key(),
        constraint = worker.is_active @ GrapheneError::WorkerNotRegistered
    )]
    pub worker: Account<'info, WorkerRegistry>,
}

pub fn unregister_worker(ctx: Context<UnregisterWorker>) -> Result<()> {
    let worker = &mut ctx.accounts.worker;

    // Mark as inactive (unbonding period would be enforced off-chain or in separate instruction)
    worker.is_active = false;

    msg!("Worker unregistered: authority={}", worker.authority);

    // TODO: Implement 14-day unbonding period before stake can be withdrawn
    // This would require a separate withdraw_stake instruction that checks timestamp

    Ok(())
}
