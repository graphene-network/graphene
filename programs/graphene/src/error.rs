use anchor_lang::prelude::*;

#[error_code]
pub enum GrapheneError {
    #[msg("Invalid channel state for this operation")]
    InvalidChannelState,
    #[msg("Insufficient balance in channel")]
    InsufficientBalance,
    #[msg("Invalid signature")]
    InvalidSignature,
    #[msg("Worker already registered")]
    WorkerAlreadyRegistered,
    #[msg("Worker not registered or inactive")]
    WorkerNotRegistered,
    #[msg("Unauthorized worker")]
    UnauthorizedWorker,
    #[msg("Dispute window is still active")]
    DisputeWindowActive,
    #[msg("Nonce must be greater than last settled nonce")]
    InvalidNonce,
    #[msg("Insufficient stake for worker capabilities")]
    InsufficientStake,
    #[msg("Worker is not in Active state")]
    WorkerNotActive,
    #[msg("Worker is not in Unbonding state")]
    WorkerNotUnbonding,
    #[msg("Unbonding period not complete (14 days required)")]
    UnbondingNotComplete,
}
