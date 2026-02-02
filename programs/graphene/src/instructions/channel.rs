use anchor_lang::prelude::*;
use anchor_lang::solana_program::sysvar::instructions as ix_sysvar;
use anchor_spl::token_interface::{
    close_account, transfer_checked, CloseAccount, Mint, TokenAccount, TokenInterface,
    TransferChecked,
};

use crate::error::GrapheneError;
use crate::state::{ChannelState, PaymentChannel};
use crate::utils::verify_ed25519_signature;

/// 24 hours in seconds
const DISPUTE_TIMEOUT: i64 = 86400;

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
        init,
        payer = user,
        seeds = [b"vault", channel.key().as_ref()],
        bump,
        token::mint = mint,
        token::authority = vault,
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

pub fn open_channel(ctx: Context<OpenChannel>, amount: u64) -> Result<()> {
    let channel = &mut ctx.accounts.channel;

    channel.user = ctx.accounts.user.key();
    channel.worker = ctx.accounts.worker.key();
    channel.mint = ctx.accounts.mint.key();
    channel.balance = amount;
    channel.spent = 0;
    channel.last_nonce = 0;
    channel.timeout = 0;
    channel.state = ChannelState::Open;
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

    msg!(
        "Channel opened: user={}, worker={}, mint={}, amount={}",
        channel.user,
        channel.worker,
        channel.mint,
        amount
    );

    Ok(())
}

/// Top up an existing payment channel
#[derive(Accounts)]
pub struct TopUpChannel<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        seeds = [b"channel", user.key().as_ref(), channel.worker.as_ref()],
        bump = channel.bump,
        constraint = channel.user == user.key(),
        constraint = channel.state == ChannelState::Open @ GrapheneError::InvalidChannelState
    )]
    pub channel: Account<'info, PaymentChannel>,

    #[account(
        constraint = mint.key() == channel.mint
    )]
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
}

pub fn top_up_channel(ctx: Context<TopUpChannel>, amount: u64) -> Result<()> {
    let channel = &mut ctx.accounts.channel;

    channel.balance = channel.balance.checked_add(amount).unwrap();

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

    msg!(
        "Channel topped up: amount={}, new_balance={}",
        amount,
        channel.balance
    );

    Ok(())
}

/// Initiate channel closure (starts 24h dispute window)
#[derive(Accounts)]
pub struct InitiateClose<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        seeds = [b"channel", user.key().as_ref(), channel.worker.as_ref()],
        bump = channel.bump,
        constraint = channel.user == user.key(),
        constraint = channel.state == ChannelState::Open @ GrapheneError::InvalidChannelState
    )]
    pub channel: Account<'info, PaymentChannel>,
}

pub fn initiate_close(ctx: Context<InitiateClose>) -> Result<()> {
    let channel = &mut ctx.accounts.channel;
    let clock = Clock::get()?;

    channel.state = ChannelState::Closing;
    channel.timeout = clock.unix_timestamp + DISPUTE_TIMEOUT;

    msg!("Channel closing initiated: timeout={}", channel.timeout);

    Ok(())
}

/// Force close a channel after timeout expires (user reclaims remaining funds)
#[derive(Accounts)]
pub struct ForceClose<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        seeds = [b"channel", user.key().as_ref(), channel.worker.as_ref()],
        bump = channel.bump,
        constraint = channel.user == user.key(),
        constraint = channel.state == ChannelState::Closing @ GrapheneError::InvalidChannelState,
        close = user
    )]
    pub channel: Account<'info, PaymentChannel>,

    #[account(
        constraint = mint.key() == channel.mint
    )]
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
        constraint = user_token_account.owner == user.key(),
        constraint = user_token_account.mint == mint.key()
    )]
    pub user_token_account: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Interface<'info, TokenInterface>,
}

pub fn force_close(ctx: Context<ForceClose>) -> Result<()> {
    let channel = &ctx.accounts.channel;
    let clock = Clock::get()?;

    // Verify timeout has expired
    require!(
        clock.unix_timestamp >= channel.timeout,
        GrapheneError::TimeoutNotExpired
    );

    let remaining = channel.balance.saturating_sub(channel.spent);
    let channel_key = channel.key();

    // Transfer remaining tokens to user
    if remaining > 0 {
        let seeds = &[b"vault".as_ref(), channel_key.as_ref(), &[ctx.bumps.vault]];
        let signer = &[&seeds[..]];

        let cpi_accounts = TransferChecked {
            from: ctx.accounts.vault.to_account_info(),
            to: ctx.accounts.user_token_account.to_account_info(),
            authority: ctx.accounts.vault.to_account_info(),
            mint: ctx.accounts.mint.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        transfer_checked(cpi_ctx, remaining, ctx.accounts.mint.decimals)?;
    }

    // Close the vault account
    let seeds = &[b"vault".as_ref(), channel_key.as_ref(), &[ctx.bumps.vault]];
    let signer = &[&seeds[..]];

    let cpi_accounts = CloseAccount {
        account: ctx.accounts.vault.to_account_info(),
        destination: ctx.accounts.user.to_account_info(),
        authority: ctx.accounts.vault.to_account_info(),
    };
    let cpi_program = ctx.accounts.token_program.to_account_info();
    let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
    close_account(cpi_ctx)?;

    msg!("Channel force closed: returned {} to user", remaining);

    Ok(())
}

/// Cooperative close - both user and worker sign for immediate settlement
#[derive(Accounts)]
pub struct CooperativeClose<'info> {
    pub user: Signer<'info>,
    pub worker: Signer<'info>,

    #[account(
        mut,
        seeds = [b"channel", user.key().as_ref(), worker.key().as_ref()],
        bump = channel.bump,
        constraint = channel.user == user.key(),
        constraint = channel.worker == worker.key(),
        constraint = channel.state == ChannelState::Open @ GrapheneError::InvalidChannelState,
        close = user
    )]
    pub channel: Account<'info, PaymentChannel>,

    #[account(
        constraint = mint.key() == channel.mint
    )]
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
        constraint = user_token_account.owner == user.key(),
        constraint = user_token_account.mint == mint.key()
    )]
    pub user_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        constraint = worker_token_account.owner == worker.key(),
        constraint = worker_token_account.mint == mint.key()
    )]
    pub worker_token_account: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Interface<'info, TokenInterface>,
}

pub fn cooperative_close(ctx: Context<CooperativeClose>, final_spent: u64) -> Result<()> {
    let channel = &ctx.accounts.channel;

    // final_spent must be >= current spent (can't reduce worker's earnings)
    require!(final_spent >= channel.spent, GrapheneError::InvalidNonce);

    // Can't spend more than balance
    require!(
        final_spent <= channel.balance,
        GrapheneError::InsufficientBalance
    );

    let worker_amount = final_spent;
    let user_amount = channel.balance.saturating_sub(final_spent);
    let channel_key = channel.key();

    let seeds = &[b"vault".as_ref(), channel_key.as_ref(), &[ctx.bumps.vault]];
    let signer = &[&seeds[..]];

    // Transfer worker's portion
    if worker_amount > 0 {
        let cpi_accounts = TransferChecked {
            from: ctx.accounts.vault.to_account_info(),
            to: ctx.accounts.worker_token_account.to_account_info(),
            authority: ctx.accounts.vault.to_account_info(),
            mint: ctx.accounts.mint.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        transfer_checked(cpi_ctx, worker_amount, ctx.accounts.mint.decimals)?;
    }

    // Transfer user's remaining portion
    if user_amount > 0 {
        let cpi_accounts = TransferChecked {
            from: ctx.accounts.vault.to_account_info(),
            to: ctx.accounts.user_token_account.to_account_info(),
            authority: ctx.accounts.vault.to_account_info(),
            mint: ctx.accounts.mint.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        transfer_checked(cpi_ctx, user_amount, ctx.accounts.mint.decimals)?;
    }

    // Close the vault account
    let cpi_accounts = CloseAccount {
        account: ctx.accounts.vault.to_account_info(),
        destination: ctx.accounts.user.to_account_info(),
        authority: ctx.accounts.vault.to_account_info(),
    };
    let cpi_program = ctx.accounts.token_program.to_account_info();
    let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
    close_account(cpi_ctx)?;

    msg!(
        "Channel cooperatively closed: worker={}, user={}",
        worker_amount,
        user_amount
    );

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
        constraint = channel.state == ChannelState::Open || channel.state == ChannelState::Closing @ GrapheneError::InvalidChannelState
    )]
    pub channel: Account<'info, PaymentChannel>,

    #[account(
        constraint = mint.key() == channel.mint
    )]
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

    /// CHECK: Instructions sysvar for Ed25519 verification
    #[account(address = ix_sysvar::ID)]
    pub ix_sysvar: AccountInfo<'info>,

    pub token_program: Interface<'info, TokenInterface>,
}

pub fn settle_channel(ctx: Context<SettleChannel>, amount: u64, nonce: u64) -> Result<()> {
    // Get channel key and user pubkey before mutable borrow
    let channel_key = ctx.accounts.channel.key();
    let user_pubkey = ctx.accounts.channel.user;

    // Verify Ed25519 signature from user
    verify_ed25519_signature(
        &ctx.accounts.ix_sysvar,
        &user_pubkey,
        &channel_key,
        amount,
        nonce,
    )?;

    let channel = &mut ctx.accounts.channel;

    // Validate nonce is greater than last settled
    require!(nonce > channel.last_nonce, GrapheneError::InvalidNonce);

    // Calculate new cumulative spent
    let new_spent = channel.spent.checked_add(amount).unwrap();

    // Validate cumulative spent doesn't exceed balance
    require!(
        new_spent <= channel.balance,
        GrapheneError::InsufficientBalance
    );

    // Update channel state
    channel.spent = new_spent;
    channel.last_nonce = nonce;

    // Transfer tokens from vault to worker
    let seeds = &[b"vault".as_ref(), channel_key.as_ref(), &[ctx.bumps.vault]];
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

    msg!(
        "Settlement: amount={}, nonce={}, total_spent={}",
        amount,
        nonce,
        channel.spent
    );

    Ok(())
}
