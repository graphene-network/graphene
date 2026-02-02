use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token_interface::{
    transfer_checked, CloseAccount, Mint, TokenAccount, TokenInterface, TransferChecked,
    close_account,
};

use crate::error::GrapheneError;
use crate::state::{
    calculate_min_stake, WorkerCapabilities, WorkerRegistry, WorkerState, UNBONDING_PERIOD,
};

/// Register a new worker with SPL token stake
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
    pub worker_registry: Account<'info, WorkerRegistry>,

    /// $GRAPHENE token mint
    pub mint: InterfaceAccount<'info, Mint>,

    /// Worker's token account (source of stake)
    #[account(
        mut,
        constraint = authority_token_account.owner == authority.key(),
        constraint = authority_token_account.mint == mint.key()
    )]
    pub authority_token_account: InterfaceAccount<'info, TokenAccount>,

    /// Stake escrow PDA token account
    #[account(
        init,
        payer = authority,
        seeds = [b"stake_escrow", worker_registry.key().as_ref()],
        bump,
        token::mint = mint,
        token::authority = stake_escrow,
    )]
    pub stake_escrow: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn register_worker(
    ctx: Context<RegisterWorker>,
    stake_amount: u64,
    capabilities: WorkerCapabilities,
) -> Result<()> {
    // Validate stake meets minimum for capabilities
    let min_stake = calculate_min_stake(&capabilities);
    require!(stake_amount >= min_stake, GrapheneError::InsufficientStake);

    let authority_key = ctx.accounts.authority.key();
    let mint_key = ctx.accounts.mint.key();
    let decimals = ctx.accounts.mint.decimals;

    // Transfer stake tokens to escrow
    let cpi_accounts = TransferChecked {
        from: ctx.accounts.authority_token_account.to_account_info(),
        to: ctx.accounts.stake_escrow.to_account_info(),
        authority: ctx.accounts.authority.to_account_info(),
        mint: ctx.accounts.mint.to_account_info(),
    };
    let cpi_program = ctx.accounts.token_program.to_account_info();
    let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
    transfer_checked(cpi_ctx, stake_amount, decimals)?;

    // Initialize worker registry
    let worker = &mut ctx.accounts.worker_registry;
    worker.authority = authority_key;
    worker.stake_amount = stake_amount;
    worker.stake_mint = mint_key;
    worker.registered_at = Clock::get()?.unix_timestamp;
    worker.state = WorkerState::Active;
    worker.unbonding_start = 0;
    worker.capabilities = capabilities;
    worker.bump = ctx.bumps.worker_registry;

    msg!(
        "Worker registered: authority={}, stake={}, vcpu={}, memory_mb={}",
        authority_key,
        stake_amount,
        capabilities.max_vcpu,
        capabilities.max_memory_mb
    );

    Ok(())
}

/// Initiate unbonding - starts 14-day countdown
#[derive(Accounts)]
pub struct InitiateUnbonding<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        seeds = [b"worker", authority.key().as_ref()],
        bump = worker_registry.bump,
        constraint = worker_registry.authority == authority.key(),
        constraint = worker_registry.state == WorkerState::Active @ GrapheneError::WorkerNotActive
    )]
    pub worker_registry: Account<'info, WorkerRegistry>,
}

pub fn initiate_unbonding(ctx: Context<InitiateUnbonding>) -> Result<()> {
    let worker = &mut ctx.accounts.worker_registry;
    let now = Clock::get()?.unix_timestamp;

    worker.state = WorkerState::Unbonding;
    worker.unbonding_start = now;

    msg!(
        "Unbonding initiated: authority={}, unbonding_start={}",
        worker.authority,
        now
    );

    Ok(())
}

/// Complete unbonding after 14-day period - returns stake and closes account
#[derive(Accounts)]
pub struct CompleteUnbonding<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        mut,
        seeds = [b"worker", authority.key().as_ref()],
        bump = worker_registry.bump,
        constraint = worker_registry.authority == authority.key(),
        constraint = worker_registry.state == WorkerState::Unbonding @ GrapheneError::WorkerNotUnbonding,
        close = authority
    )]
    pub worker_registry: Account<'info, WorkerRegistry>,

    /// $GRAPHENE token mint
    #[account(
        constraint = mint.key() == worker_registry.stake_mint
    )]
    pub mint: InterfaceAccount<'info, Mint>,

    /// Worker's token account (destination for returned stake)
    #[account(
        mut,
        constraint = authority_token_account.owner == authority.key(),
        constraint = authority_token_account.mint == mint.key()
    )]
    pub authority_token_account: InterfaceAccount<'info, TokenAccount>,

    /// Stake escrow PDA token account
    #[account(
        mut,
        seeds = [b"stake_escrow", worker_registry.key().as_ref()],
        bump,
        constraint = stake_escrow.mint == mint.key()
    )]
    pub stake_escrow: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Interface<'info, TokenInterface>,
}

pub fn complete_unbonding(ctx: Context<CompleteUnbonding>) -> Result<()> {
    let worker = &ctx.accounts.worker_registry;
    let now = Clock::get()?.unix_timestamp;

    // Verify 14-day unbonding period has elapsed
    require!(
        now >= worker.unbonding_start + UNBONDING_PERIOD,
        GrapheneError::UnbondingNotComplete
    );

    let worker_registry_key = ctx.accounts.worker_registry.key();
    let stake_amount = worker.stake_amount;
    let decimals = ctx.accounts.mint.decimals;

    // Build signer seeds for stake_escrow PDA
    let seeds = &[
        b"stake_escrow",
        worker_registry_key.as_ref(),
        &[ctx.bumps.stake_escrow],
    ];
    let signer = &[&seeds[..]];

    // Transfer stake back to authority
    let cpi_accounts = TransferChecked {
        from: ctx.accounts.stake_escrow.to_account_info(),
        to: ctx.accounts.authority_token_account.to_account_info(),
        authority: ctx.accounts.stake_escrow.to_account_info(),
        mint: ctx.accounts.mint.to_account_info(),
    };
    let cpi_program = ctx.accounts.token_program.to_account_info();
    let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
    transfer_checked(cpi_ctx, stake_amount, decimals)?;

    // Close the stake escrow account, return rent to authority
    let close_accounts = CloseAccount {
        account: ctx.accounts.stake_escrow.to_account_info(),
        destination: ctx.accounts.authority.to_account_info(),
        authority: ctx.accounts.stake_escrow.to_account_info(),
    };
    let cpi_program = ctx.accounts.token_program.to_account_info();
    let cpi_ctx = CpiContext::new_with_signer(cpi_program, close_accounts, signer);
    close_account(cpi_ctx)?;

    msg!(
        "Unbonding complete: authority={}, stake_returned={}",
        worker.authority,
        stake_amount
    );

    Ok(())
}
