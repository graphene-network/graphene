use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface, TransferChecked, transfer_checked};

use crate::error::GrapheneError;
use crate::state::{ChannelStatus, PaymentChannel};

/// Open a new payment channel with a worker
#[derive(Accounts)]
pub struct OpenChannel<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    /// CHECK: Worker pubkey, validated in instruction
    pub worker: UncheckedAccount<'info>,

    #[account(
        init,
        payer = user,
        space = PaymentChannel::LEN,
        seeds = [b"channel", user.key().as_ref(), worker.key().as_ref()],
        bump
    )]
    pub channel: Account<'info, PaymentChannel>,

    pub mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        constraint = user_token_account.owner == user.key(),
        constraint = user_token_account.mint == mint.key()
    )]
    pub user_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [b"vault", channel.key().as_ref()],
        bump,
        constraint = vault.mint == mint.key()
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

pub fn open_channel(ctx: Context<OpenChannel>, amount: u64) -> Result<()> {
    let channel = &mut ctx.accounts.channel;

    channel.user = ctx.accounts.user.key();
    channel.worker = ctx.accounts.worker.key();
    channel.balance = amount;
    channel.nonce = 0;
    channel.status = ChannelStatus::Open;
    channel.dispute_deadline = 0;
    channel.last_settlement = Clock::get()?.unix_timestamp;
    channel.bump = ctx.bumps.channel;

    // Transfer tokens to vault
    let cpi_accounts = TransferChecked {
        from: ctx.accounts.user_token_account.to_account_info(),
        to: ctx.accounts.vault.to_account_info(),
        authority: ctx.accounts.user.to_account_info(),
        mint: ctx.accounts.mint.to_account_info(),
    };
    let cpi_program = ctx.accounts.token_program.to_account_info();
    let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
    transfer_checked(cpi_ctx, amount, ctx.accounts.mint.decimals)?;

    msg!("Channel opened: user={}, worker={}, amount={}",
        channel.user, channel.worker, amount);

    Ok(())
}

/// Close a payment channel (initiates dispute window)
#[derive(Accounts)]
pub struct CloseChannel<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        seeds = [b"channel", user.key().as_ref(), channel.worker.as_ref()],
        bump = channel.bump,
        constraint = channel.user == user.key(),
        constraint = channel.status == ChannelStatus::Open @ GrapheneError::InvalidChannelState
    )]
    pub channel: Account<'info, PaymentChannel>,
}

pub fn close_channel(ctx: Context<CloseChannel>) -> Result<()> {
    let channel = &mut ctx.accounts.channel;
    let clock = Clock::get()?;

    // 24-hour dispute window
    channel.status = ChannelStatus::Disputing;
    channel.dispute_deadline = clock.unix_timestamp + 86400;

    msg!("Channel closing initiated: dispute deadline={}", channel.dispute_deadline);

    Ok(())
}

/// Settle a channel - worker claims payment with Ed25519 ticket
#[derive(Accounts)]
pub struct SettleChannel<'info> {
    #[account(mut)]
    pub worker: Signer<'info>,

    #[account(
        mut,
        seeds = [b"channel", channel.user.as_ref(), worker.key().as_ref()],
        bump = channel.bump,
        constraint = channel.worker == worker.key(),
        constraint = channel.status == ChannelStatus::Open || channel.status == ChannelStatus::Disputing @ GrapheneError::InvalidChannelState
    )]
    pub channel: Account<'info, PaymentChannel>,

    pub mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        seeds = [b"vault", channel.key().as_ref()],
        bump,
        constraint = vault.mint == mint.key()
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        constraint = worker_token_account.owner == worker.key(),
        constraint = worker_token_account.mint == mint.key()
    )]
    pub worker_token_account: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Interface<'info, TokenInterface>,
}

pub fn settle_channel(ctx: Context<SettleChannel>, amount: u64, nonce: u64) -> Result<()> {
    // Get channel key before mutable borrow
    let channel_key = ctx.accounts.channel.key();

    let channel = &mut ctx.accounts.channel;

    // Validate nonce is greater than last settled
    require!(nonce > channel.nonce, GrapheneError::InvalidNonce);

    // Validate amount doesn't exceed balance
    require!(amount <= channel.balance, GrapheneError::InsufficientBalance);

    // TODO: Ed25519 signature verification via instruction introspection
    // This should verify that the user signed: (channel_pubkey, amount, nonce)
    // using Solana's Ed25519 program precompile
    msg!("TODO: Implement Ed25519 signature verification");

    // Update channel state
    channel.balance = channel.balance.saturating_sub(amount);
    channel.nonce = nonce;
    channel.last_settlement = Clock::get()?.unix_timestamp;

    let remaining_balance = channel.balance;

    // Transfer tokens from vault to worker
    let seeds = &[
        b"vault",
        channel_key.as_ref(),
        &[ctx.bumps.vault],
    ];
    let signer = &[&seeds[..]];

    let cpi_accounts = TransferChecked {
        from: ctx.accounts.vault.to_account_info(),
        to: ctx.accounts.worker_token_account.to_account_info(),
        authority: ctx.accounts.vault.to_account_info(),
        mint: ctx.accounts.mint.to_account_info(),
    };
    let cpi_program = ctx.accounts.token_program.to_account_info();
    let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
    transfer_checked(cpi_ctx, amount, ctx.accounts.mint.decimals)?;

    msg!("Settlement: amount={}, nonce={}, remaining={}", amount, nonce, remaining_balance);

    Ok(())
}
