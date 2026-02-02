//! Solana client wrapper for worker registration and status queries.

use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{read_keypair_file, Keypair, Signer};
use solana_sdk::transaction::Transaction;
use std::str::FromStr;

use super::config::SolanaSettings;
use super::WorkerError;

/// Worker status as stored on-chain.
#[derive(Debug, Clone)]
pub struct WorkerStatus {
    /// Worker's authority (owner) public key
    pub authority: Pubkey,
    /// Amount of SOL staked (in lamports)
    pub stake: u64,
    /// Whether the worker is currently active
    pub is_active: bool,
    /// Registration timestamp (Unix epoch)
    pub registered_at: i64,
}

/// Client for interacting with the Graphene Solana program.
pub struct SolanaClient {
    rpc: RpcClient,
    keypair: Keypair,
    program_id: Pubkey,
}

impl SolanaClient {
    /// Create a new Solana client from settings.
    pub fn new(settings: &SolanaSettings) -> Result<Self, WorkerError> {
        let keypair = read_keypair_file(&settings.keypair_path).map_err(|e| {
            WorkerError::SolanaError(format!(
                "Failed to read keypair from {:?}: {}",
                settings.keypair_path, e
            ))
        })?;

        let program_id = Pubkey::from_str(&settings.program_id).map_err(|e| {
            WorkerError::SolanaError(format!(
                "Invalid program ID '{}': {}",
                settings.program_id, e
            ))
        })?;

        let rpc =
            RpcClient::new_with_commitment(settings.rpc_url.clone(), CommitmentConfig::confirmed());

        Ok(Self {
            rpc,
            keypair,
            program_id,
        })
    }

    /// Get the worker's public key (authority).
    pub fn authority(&self) -> Pubkey {
        self.keypair.pubkey()
    }

    /// Derive the worker registry PDA for this authority.
    pub fn worker_pda(&self) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[b"worker", self.authority().as_ref()], &self.program_id)
    }

    /// Compute the Anchor instruction discriminator for an instruction name.
    fn instruction_discriminator(name: &str) -> [u8; 8] {
        use sha2::{Digest, Sha256};
        let preimage = format!("global:{}", name);
        let hash = Sha256::digest(preimage.as_bytes());
        let mut discriminator = [0u8; 8];
        discriminator.copy_from_slice(&hash[..8]);
        discriminator
    }

    /// Register this worker on-chain with the specified stake amount.
    ///
    /// Returns the transaction signature on success.
    pub async fn register_worker(&self, stake_lamports: u64) -> Result<String, WorkerError> {
        let (worker_pda, _bump) = self.worker_pda();

        // Build instruction data: discriminator + stake (u64 LE)
        let mut data = Self::instruction_discriminator("register_worker").to_vec();
        data.extend_from_slice(&stake_lamports.to_le_bytes());

        // System program ID
        let system_program_id = Pubkey::from_str("11111111111111111111111111111111").unwrap();

        let instruction = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(worker_pda, false),
                AccountMeta::new(self.authority(), true),
                AccountMeta::new_readonly(system_program_id, false),
            ],
            data,
        };

        let recent_blockhash = self.rpc.get_latest_blockhash()?;
        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&self.authority()),
            &[&self.keypair],
            recent_blockhash,
        );

        let signature = self.rpc.send_and_confirm_transaction(&transaction)?;
        Ok(signature.to_string())
    }

    /// Unregister this worker and reclaim stake.
    ///
    /// Returns the transaction signature on success.
    pub async fn unregister_worker(&self) -> Result<String, WorkerError> {
        let (worker_pda, _bump) = self.worker_pda();

        // Build instruction data: discriminator only
        let data = Self::instruction_discriminator("unregister_worker").to_vec();

        let instruction = Instruction {
            program_id: self.program_id,
            accounts: vec![
                AccountMeta::new(worker_pda, false),
                AccountMeta::new(self.authority(), true),
            ],
            data,
        };

        let recent_blockhash = self.rpc.get_latest_blockhash()?;
        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&self.authority()),
            &[&self.keypair],
            recent_blockhash,
        );

        let signature = self.rpc.send_and_confirm_transaction(&transaction)?;
        Ok(signature.to_string())
    }

    /// Get the current on-chain status of this worker.
    ///
    /// Returns `None` if the worker is not registered.
    pub async fn get_worker_status(&self) -> Result<Option<WorkerStatus>, WorkerError> {
        let (worker_pda, _bump) = self.worker_pda();

        // Try to fetch the account data
        match self.rpc.get_account(&worker_pda) {
            Ok(account) => {
                // Parse the account data
                // The first 8 bytes are the Anchor discriminator
                if account.data.len() < 8 + 32 + 8 + 1 + 8 {
                    return Err(WorkerError::SolanaError(
                        "Invalid worker account data".to_string(),
                    ));
                }

                let data = &account.data[8..]; // Skip discriminator

                let authority = Pubkey::try_from(&data[0..32]).map_err(|e| {
                    WorkerError::SolanaError(format!("Failed to parse authority: {}", e))
                })?;

                let stake = u64::from_le_bytes(data[32..40].try_into().unwrap());
                let is_active = data[40] != 0;
                let registered_at = i64::from_le_bytes(data[41..49].try_into().unwrap());

                Ok(Some(WorkerStatus {
                    authority,
                    stake,
                    is_active,
                    registered_at,
                }))
            }
            Err(e) => {
                // Check if account doesn't exist
                if e.to_string().contains("AccountNotFound") {
                    Ok(None)
                } else {
                    Err(WorkerError::SolanaError(format!(
                        "Failed to fetch worker account: {}",
                        e
                    )))
                }
            }
        }
    }
}
