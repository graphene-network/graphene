#![deny(clippy::all)]

//! Native bindings for the OpenCapsule SDK.
//!
//! This crate provides Node.js bindings via napi-rs for:
//! - Channel key derivation from Ed25519 identities
//! - Job encryption/decryption with forward secrecy
//! - HTTP-based job submission to OpenCapsule workers

use napi::bindgen_prelude::*;
use napi_derive::napi;

use opencapsule_node::crypto::{
    self, CryptoProvider, DefaultCryptoProvider, EncryptedBlob as RustEncryptedBlob,
    EncryptionDirection as RustEncryptionDirection,
};

/// Errors that can occur in SDK operations.
#[derive(Debug, thiserror::Error)]
pub enum SdkError {
    #[error("Invalid key length: expected {expected}, got {actual}")]
    InvalidKeyLength { expected: usize, actual: usize },

    #[error("Crypto error: {0}")]
    Crypto(String),

    #[error("Invalid direction: {0}")]
    InvalidDirection(String),

    #[error("HTTP error: {0}")]
    Http(String),
}

impl From<SdkError> for napi::Error {
    fn from(err: SdkError) -> Self {
        napi::Error::from_reason(err.to_string())
    }
}

impl From<crypto::CryptoError> for SdkError {
    fn from(err: crypto::CryptoError) -> Self {
        SdkError::Crypto(err.to_string())
    }
}

/// Direction of encryption (affects key derivation for domain separation).
#[napi]
pub enum EncryptionDirection {
    /// User encrypting input/code for worker
    Input,
    /// Worker encrypting result for user
    Output,
}

impl From<EncryptionDirection> for RustEncryptionDirection {
    fn from(dir: EncryptionDirection) -> Self {
        match dir {
            EncryptionDirection::Input => RustEncryptionDirection::Input,
            EncryptionDirection::Output => RustEncryptionDirection::Output,
        }
    }
}

/// Channel keys derived from a payment channel relationship.
///
/// Contains the shared channel master key and X25519 keys for
/// per-job ephemeral key exchanges.
#[napi]
pub struct ChannelKeys {
    inner: crypto::ChannelKeys,
}

#[napi]
impl ChannelKeys {
    /// Get the channel master key (32 bytes).
    ///
    /// This key is shared between both parties in the payment channel.
    #[napi]
    pub fn master_key(&self) -> Buffer {
        Buffer::from(self.inner.master_key().to_vec())
    }

    /// Get the peer's X25519 public key (32 bytes).
    #[napi]
    pub fn peer_public_key(&self) -> Buffer {
        Buffer::from(self.inner.peer_x25519_public().as_bytes().to_vec())
    }
}

/// An encrypted blob containing all data needed for decryption.
///
/// Format: version (1 byte) + ephemeral pubkey (32 bytes) + nonce (24 bytes) + ciphertext
#[napi]
pub struct EncryptedBlob {
    inner: RustEncryptedBlob,
}

#[napi]
impl EncryptedBlob {
    /// Get the format version.
    #[napi(getter)]
    pub fn version(&self) -> u8 {
        self.inner.version
    }

    /// Get the ephemeral X25519 public key used for this encryption (32 bytes).
    #[napi(getter)]
    pub fn ephemeral_pubkey(&self) -> Buffer {
        Buffer::from(self.inner.ephemeral_pubkey.to_vec())
    }

    /// Get the 192-bit nonce (24 bytes).
    #[napi(getter)]
    pub fn nonce(&self) -> Buffer {
        Buffer::from(self.inner.nonce.to_vec())
    }

    /// Get the ciphertext with authentication tag.
    #[napi(getter)]
    pub fn ciphertext(&self) -> Buffer {
        Buffer::from(self.inner.ciphertext.clone())
    }

    /// Serialize the blob to bytes for transmission/storage.
    #[napi]
    pub fn to_bytes(&self) -> Buffer {
        Buffer::from(self.inner.to_bytes())
    }

    /// Deserialize a blob from bytes.
    #[napi(factory)]
    pub fn from_bytes(bytes: Buffer) -> Result<EncryptedBlob> {
        let inner = RustEncryptedBlob::from_bytes(&bytes)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(EncryptedBlob { inner })
    }
}

/// Derive channel keys from Ed25519 identities and a shared channel identifier.
///
/// This performs:
/// 1. Ed25519 -> X25519 key conversion
/// 2. X25519 ECDH between local secret and peer public
/// 3. HKDF with channel ID as salt to derive the channel master key
///
/// Both parties will derive the same master key.
///
/// # Arguments
/// * `local_secret` - Your Ed25519 secret key (32 bytes)
/// * `peer_pubkey` - Peer's Ed25519 public key (32 bytes)
/// * `channel_id` - Shared channel identifier (32 bytes)
///
/// # Returns
/// ChannelKeys containing the shared master key and X25519 keys
#[napi]
pub fn derive_channel_keys(
    local_secret: Buffer,
    peer_pubkey: Buffer,
    channel_id: Buffer,
) -> Result<ChannelKeys> {
    // Validate input lengths
    if local_secret.len() != 32 {
        return Err(SdkError::InvalidKeyLength {
            expected: 32,
            actual: local_secret.len(),
        }
        .into());
    }
    if peer_pubkey.len() != 32 {
        return Err(SdkError::InvalidKeyLength {
            expected: 32,
            actual: peer_pubkey.len(),
        }
        .into());
    }
    if channel_id.len() != 32 {
        return Err(SdkError::InvalidKeyLength {
            expected: 32,
            actual: channel_id.len(),
        }
        .into());
    }

    // Convert buffers to fixed-size arrays
    let local_secret_arr: [u8; 32] = local_secret
        .as_ref()
        .try_into()
        .expect("length already validated");
    let peer_pubkey_arr: [u8; 32] = peer_pubkey
        .as_ref()
        .try_into()
        .expect("length already validated");
    let channel_id_arr: [u8; 32] = channel_id
        .as_ref()
        .try_into()
        .expect("length already validated");

    let provider = DefaultCryptoProvider;
    let inner = provider
        .derive_channel_keys(&local_secret_arr, &peer_pubkey_arr, &channel_id_arr)
        .map_err(SdkError::from)?;

    Ok(ChannelKeys { inner })
}

/// Encrypt data for a job using channel keys.
///
/// Uses XChaCha20-Poly1305 with per-job ephemeral keys for forward secrecy.
/// The job_id is incorporated into key derivation to prevent cross-job decryption.
///
/// # Arguments
/// * `plaintext` - Data to encrypt
/// * `channel_keys` - Pre-derived channel keys
/// * `job_id` - Unique job identifier
/// * `direction` - Input (user->worker) or Output (worker->user)
///
/// # Returns
/// EncryptedBlob containing version, ephemeral pubkey, nonce, and ciphertext
#[napi]
pub fn encrypt_job_blob(
    plaintext: Buffer,
    channel_keys: &ChannelKeys,
    job_id: String,
    direction: EncryptionDirection,
) -> Result<EncryptedBlob> {
    let provider = DefaultCryptoProvider;
    let inner = provider
        .encrypt_job_blob(&plaintext, &channel_keys.inner, &job_id, direction.into())
        .map_err(SdkError::from)?;

    Ok(EncryptedBlob { inner })
}

/// Decrypt a job blob using channel keys.
///
/// # Arguments
/// * `encrypted` - The encrypted blob
/// * `channel_keys` - Pre-derived channel keys (from the receiving side)
/// * `job_id` - Unique job identifier (must match encryption)
/// * `direction` - Must match the direction used during encryption
///
/// # Returns
/// Decrypted plaintext
#[napi]
pub fn decrypt_job_blob(
    encrypted: &EncryptedBlob,
    channel_keys: &ChannelKeys,
    job_id: String,
    direction: EncryptionDirection,
) -> Result<Buffer> {
    let provider = DefaultCryptoProvider;
    let plaintext = provider
        .decrypt_job_blob(
            &encrypted.inner,
            &channel_keys.inner,
            &job_id,
            direction.into(),
        )
        .map_err(SdkError::from)?;

    Ok(Buffer::from(plaintext))
}

// ============================================================================
// Hashing Utilities
// ============================================================================

/// Compute the BLAKE3 hash of the given data.
///
/// Returns the 32-byte hash as a Buffer.
#[napi]
pub fn blake3_hash(data: Buffer) -> Buffer {
    let hash = blake3::hash(&data);
    Buffer::from(hash.as_bytes().to_vec())
}

// ============================================================================
// HTTP Client Types
// ============================================================================

use opencapsule_node::api::{
    ApiError as RustApiError, JobResultResponse, JobStatusResponse, SubmitJobRequest,
    SubmitJobResponse,
};
use opencapsule_node::types::{
    AssetData, Compression, EgressRule as RustEgressRule, JobAssets as RustJobAssets,
    JobManifest as RustJobManifest,
};
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;

/// An egress rule specifying an allowed outbound connection.
#[napi(object)]
pub struct EgressRule {
    /// Hostname or IP address.
    pub host: String,
    /// Port number.
    pub port: u32,
    /// Protocol (tcp/udp).
    pub protocol: String,
}

/// Resource usage metrics for a completed job.
#[napi(object)]
pub struct JobMetrics {
    /// Peak memory usage in bytes.
    pub peak_memory_bytes: BigInt,
    /// Total CPU time in milliseconds.
    pub cpu_time_ms: BigInt,
    /// Total network bytes received.
    pub network_rx_bytes: BigInt,
    /// Total network bytes transmitted.
    pub network_tx_bytes: BigInt,
}

/// Status of a job.
#[napi(string_enum)]
pub enum JobStatus {
    /// Job accepted and queued for execution.
    Accepted,
    /// Job is currently running.
    Running,
    /// Job completed successfully (exit code 0).
    Succeeded,
    /// Job failed (non-zero exit code).
    Failed,
    /// Job exceeded time limit.
    Timeout,
    /// Job was rejected (check reject_reason field).
    Rejected,
}

/// Reason for job rejection.
#[napi(string_enum)]
pub enum RejectReason {
    /// Worker is at capacity.
    CapacityFull,
    /// Requested kernel is not supported.
    UnsupportedRuntime,
    /// Requested resources exceed worker limits.
    ResourcesExceedLimits,
    /// Environment variables total size exceeds limit.
    EnvTooLarge,
    /// Environment variable name is invalid.
    InvalidEnvName,
    /// Environment variable uses reserved OPENCAPSULE_* prefix.
    ReservedEnvPrefix,
    /// Code or input blob could not be fetched.
    AssetUnavailable,
    /// Inline asset exceeds maximum allowed size.
    InlineTooLarge,
    /// Generic internal error.
    InternalError,
}

/// Configuration for creating a OpenCapsule client.
#[napi(object)]
pub struct ClientConfig {
    /// Worker HTTP URL (e.g., "http://192.168.1.100:3000").
    pub worker_url: String,
    /// Your Ed25519 secret key (32 bytes).
    pub secret_key: Buffer,
    /// Shared channel identifier (32 bytes) - used for key derivation.
    pub channel_id: Buffer,
    /// Worker's Ed25519 public key (hex-encoded, 64 hex chars).
    /// Used for channel key derivation and encryption.
    pub worker_pubkey: String,
}

/// Resource requirements for a job.
#[napi(object)]
pub struct ResourceOptions {
    /// Number of vCPUs (default: 1).
    pub vcpu: Option<u32>,
    /// Memory in MB (default: 256).
    pub memory_mb: Option<u32>,
}

/// Networking options for a job.
#[napi(object)]
pub struct NetworkingOptions {
    /// Estimated network ingress in megabytes.
    pub estimated_ingress_mb: Option<BigInt>,
    /// Estimated network egress in megabytes.
    pub estimated_egress_mb: Option<BigInt>,
    /// Egress allowlist.
    pub egress_allowlist: Option<Vec<EgressRule>>,
}

/// Asset delivery options for a job.
#[napi(object)]
#[derive(Default)]
pub struct AssetOptions {
    /// Enable zstd compression before encryption.
    pub compress: Option<bool>,
}

/// Options for submitting a job.
#[napi(object)]
pub struct JobOptions {
    /// Code to execute (UTF-8 string).
    pub code: String,
    /// Optional input data.
    pub input: Option<Buffer>,
    /// Resource requirements (vCPU, memory).
    pub resources: Option<ResourceOptions>,
    /// Networking options (egress allowlist, bandwidth estimates).
    pub networking: Option<NetworkingOptions>,
    /// Asset delivery options (compression).
    pub assets: Option<AssetOptions>,
    /// Timeout in milliseconds (default: 30000).
    pub timeout_ms: Option<BigInt>,
    /// Runtime to use (default: "python:3.12").
    pub runtime: Option<String>,
    /// Environment variables.
    pub env: Option<HashMap<String, String>>,
}

/// Result from a completed job.
#[napi(object)]
pub struct NativeJobResult {
    /// Exit code (0 = success).
    pub exit_code: i32,
    /// Decrypted output data.
    pub output: Buffer,
    /// Execution duration in milliseconds.
    pub duration_ms: BigInt,
    /// Resource usage metrics.
    pub metrics: JobMetrics,
}

// ============================================================================
// HTTP-Based Client
// ============================================================================

/// A native OpenCapsule network client using HTTP transport.
///
/// Handles:
/// - Channel key derivation for end-to-end encryption
/// - Job encryption/decryption
/// - HTTP-based job submission and result retrieval
#[napi]
pub struct OpenCapsuleClient {
    client: reqwest::Client,
    base_url: String,
    channel_keys: crypto::ChannelKeys,
    #[allow(dead_code)]
    secret_key: [u8; 32],
    #[allow(dead_code)]
    channel_id: [u8; 32],
    nonce: AtomicU64,
    cumulative_amount: AtomicU64,
}

#[napi]
impl OpenCapsuleClient {
    /// Create a new OpenCapsule client.
    ///
    /// This initializes:
    /// - Channel key derivation for end-to-end encryption
    /// - HTTP client for job submission
    ///
    /// # Arguments
    /// * `config` - Client configuration with keys and worker URL
    #[napi(factory)]
    pub async fn create(config: ClientConfig) -> Result<OpenCapsuleClient> {
        // Validate key lengths
        if config.secret_key.len() != 32 {
            return Err(napi::Error::from_reason(format!(
                "secret_key must be 32 bytes, got {}",
                config.secret_key.len()
            )));
        }
        if config.channel_id.len() != 32 {
            return Err(napi::Error::from_reason(format!(
                "channel_id must be 32 bytes, got {}",
                config.channel_id.len()
            )));
        }

        // Parse worker pubkey (hex-encoded Ed25519 public key)
        let worker_pubkey: [u8; 32] = hex::decode(&config.worker_pubkey)
            .map_err(|e| napi::Error::from_reason(format!("Invalid worker_pubkey hex: {}", e)))?
            .try_into()
            .map_err(|_| {
                napi::Error::from_reason(
                    "worker_pubkey must be 64 hex characters (32 bytes)".to_string(),
                )
            })?;

        // Convert to fixed arrays
        let secret_key: [u8; 32] = config.secret_key.as_ref().try_into().unwrap();
        let channel_id: [u8; 32] = config.channel_id.as_ref().try_into().unwrap();

        // Derive channel keys
        let provider = DefaultCryptoProvider;
        let channel_keys = provider
            .derive_channel_keys(&secret_key, &worker_pubkey, &channel_id)
            .map_err(|e| {
                napi::Error::from_reason(format!("Failed to derive channel keys: {}", e))
            })?;

        // Normalize base URL (strip trailing slash)
        let base_url = config.worker_url.trim_end_matches('/').to_string();

        Ok(OpenCapsuleClient {
            client: reqwest::Client::new(),
            base_url,
            channel_keys,
            secret_key,
            channel_id,
            nonce: AtomicU64::new(0),
            cumulative_amount: AtomicU64::new(0),
        })
    }

    /// Submit a job to the worker.
    ///
    /// This handles:
    /// 1. Generates unique job ID
    /// 2. Encrypts code and optional input
    /// 3. Submits via HTTP POST
    /// 4. Polls for completion
    /// 5. Retrieves and decrypts result
    ///
    /// # Arguments
    /// * `options` - Job configuration
    ///
    /// # Returns
    /// Job result with decrypted output
    #[napi]
    pub async fn submit_job(&self, options: JobOptions) -> Result<NativeJobResult> {
        use std::sync::atomic::Ordering;

        // Generate job ID
        let job_id = uuid::Uuid::new_v4().to_string();

        // Apply defaults
        let resources = options.resources.unwrap_or(ResourceOptions {
            vcpu: None,
            memory_mb: None,
        });
        let networking = options.networking.unwrap_or(NetworkingOptions {
            estimated_ingress_mb: None,
            estimated_egress_mb: None,
            egress_allowlist: None,
        });

        let vcpu = resources.vcpu.unwrap_or(1) as u8;
        let memory_mb = resources.memory_mb.unwrap_or(256);
        let timeout_ms = options.timeout_ms.map(|b| b.get_u64().1).unwrap_or(30000);
        let runtime = options
            .runtime
            .unwrap_or_else(|| "python:3.12".to_string());
        let env = options.env.unwrap_or_default();
        let egress_allowlist = networking.egress_allowlist.unwrap_or_default();
        let estimated_egress_mb = networking.estimated_egress_mb.map(|b| b.get_u64().1);
        let estimated_ingress_mb = networking.estimated_ingress_mb.map(|b| b.get_u64().1);

        let asset_opts = options.assets.unwrap_or_default();
        let compress = asset_opts.compress.unwrap_or(false);

        // Encrypt code
        let provider = DefaultCryptoProvider;
        let code_bytes = options.code.as_bytes();

        let code_to_encrypt: std::borrow::Cow<[u8]> = if compress {
            std::borrow::Cow::Owned(zstd::encode_all(code_bytes, 3).map_err(|e| {
                napi::Error::from_reason(format!("Failed to compress code: {}", e))
            })?)
        } else {
            std::borrow::Cow::Borrowed(code_bytes)
        };

        let encrypted_code = provider
            .encrypt_job_blob(
                &code_to_encrypt,
                &self.channel_keys,
                &job_id,
                RustEncryptionDirection::Input,
            )
            .map_err(|e| napi::Error::from_reason(format!("Failed to encrypt code: {}", e)))?;
        let encrypted_code_bytes = encrypted_code.to_bytes();

        // Encrypt input if provided
        let encrypted_input_bytes = if let Some(input) = &options.input {
            let input_to_encrypt: std::borrow::Cow<[u8]> = if compress {
                std::borrow::Cow::Owned(zstd::encode_all(&input[..], 3).map_err(|e| {
                    napi::Error::from_reason(format!("Failed to compress input: {}", e))
                })?)
            } else {
                std::borrow::Cow::Borrowed(&input[..])
            };

            let encrypted = provider
                .encrypt_job_blob(
                    &input_to_encrypt,
                    &self.channel_keys,
                    &job_id,
                    RustEncryptionDirection::Input,
                )
                .map_err(|e| {
                    napi::Error::from_reason(format!("Failed to encrypt input: {}", e))
                })?;
            Some(encrypted.to_bytes())
        } else {
            None
        };

        // Track nonce/amount for future billing
        let _new_nonce = self.nonce.fetch_add(1, Ordering::SeqCst) + 1;
        let cost_estimate = (vcpu as u64 * timeout_ms) + (memory_mb as u64 * timeout_ms);
        let _new_amount = self
            .cumulative_amount
            .fetch_add(cost_estimate, Ordering::SeqCst)
            + cost_estimate;

        // Build egress rules
        let egress: Vec<RustEgressRule> = egress_allowlist
            .into_iter()
            .map(|r| RustEgressRule {
                host: r.host,
                port: r.port as u16,
                protocol: r.protocol,
            })
            .collect();

        let compression = if compress {
            Compression::Zstd
        } else {
            Compression::None
        };

        // Build HTTP request body
        let manifest = RustJobManifest {
            vcpu,
            memory_mb,
            timeout_ms,
            runtime,
            egress_allowlist: egress,
            env,
            estimated_egress_mb,
            estimated_ingress_mb,
        };

        let assets = RustJobAssets {
            code: AssetData::inline(encrypted_code_bytes),
            input: encrypted_input_bytes.map(AssetData::inline),
            files: vec![],
            compression,
        };

        let submit_request = SubmitJobRequest {
            manifest,
            assets,
            encrypted_ticket: None,
        };

        // POST /v1/jobs
        let resp = self
            .client
            .post(format!("{}/v1/jobs", self.base_url))
            .json(&submit_request)
            .send()
            .await
            .map_err(|e| napi::Error::from_reason(format!("HTTP request failed: {}", e)))?;

        if !resp.status().is_success() && resp.status().as_u16() != 202 {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            if let Ok(api_err) = serde_json::from_str::<RustApiError>(&body) {
                return Err(napi::Error::from_reason(format!(
                    "Job submission failed: {}: {}",
                    api_err.code, api_err.message
                )));
            }
            return Err(napi::Error::from_reason(format!(
                "Job submission failed ({}): {}",
                status, body
            )));
        }

        let submit_resp: SubmitJobResponse = resp
            .json()
            .await
            .map_err(|e| napi::Error::from_reason(format!("Failed to parse response: {}", e)))?;

        let server_job_id = submit_resp.job_id;

        // Poll GET /v1/jobs/:id until terminal
        let poll_interval = tokio::time::Duration::from_millis(250);
        let max_poll_time = tokio::time::Duration::from_millis(timeout_ms + 30000); // job timeout + 30s buffer
        let poll_start = tokio::time::Instant::now();

        loop {
            if poll_start.elapsed() > max_poll_time {
                return Err(napi::Error::from_reason(
                    "Timed out waiting for job completion",
                ));
            }

            tokio::time::sleep(poll_interval).await;

            let status_resp = self
                .client
                .get(format!("{}/v1/jobs/{}", self.base_url, server_job_id))
                .send()
                .await
                .map_err(|e| {
                    napi::Error::from_reason(format!("Status poll failed: {}", e))
                })?;

            if !status_resp.status().is_success() {
                continue; // Retry on transient errors
            }

            let status: JobStatusResponse = status_resp.json().await.map_err(|e| {
                napi::Error::from_reason(format!("Failed to parse status: {}", e))
            })?;

            let state_str = format!("{:?}", status.state).to_lowercase();
            if state_str.contains("succeeded")
                || state_str.contains("failed")
                || state_str.contains("timeout")
            {
                break;
            }
        }

        // GET /v1/jobs/:id/result
        let result_resp = self
            .client
            .get(format!(
                "{}/v1/jobs/{}/result",
                self.base_url, server_job_id
            ))
            .send()
            .await
            .map_err(|e| napi::Error::from_reason(format!("Result fetch failed: {}", e)))?;

        if !result_resp.status().is_success() {
            let body = result_resp.text().await.unwrap_or_default();
            return Err(napi::Error::from_reason(format!(
                "Failed to fetch result: {}",
                body
            )));
        }

        let result: JobResultResponse = result_resp.json().await.map_err(|e| {
            napi::Error::from_reason(format!("Failed to parse result: {}", e))
        })?;

        // Decrypt the output (stdout is the primary output)
        let encrypted_output = if !result.stdout.is_empty() {
            result.stdout
        } else if !result.result.is_empty() {
            result.result
        } else {
            Vec::new()
        };

        let output = if encrypted_output.is_empty() {
            Vec::new()
        } else {
            let encrypted_blob =
                RustEncryptedBlob::from_bytes(&encrypted_output).map_err(|e| {
                    napi::Error::from_reason(format!("Invalid encrypted output: {}", e))
                })?;

            provider
                .decrypt_job_blob(
                    &encrypted_blob,
                    &self.channel_keys,
                    &job_id,
                    RustEncryptionDirection::Output,
                )
                .map_err(|e| {
                    napi::Error::from_reason(format!("Failed to decrypt output: {}", e))
                })?
        };

        Ok(NativeJobResult {
            exit_code: result.exit_code,
            output: Buffer::from(output),
            duration_ms: BigInt::from(result.duration_ms),
            metrics: JobMetrics {
                peak_memory_bytes: BigInt::from(0u64),
                cpu_time_ms: BigInt::from(result.duration_ms),
                network_rx_bytes: BigInt::from(0u64),
                network_tx_bytes: BigInt::from(0u64),
            },
        })
    }

    /// Get the current nonce value.
    #[napi(getter)]
    pub fn current_nonce(&self) -> BigInt {
        BigInt::from(self.nonce.load(std::sync::atomic::Ordering::SeqCst))
    }

    /// Get the cumulative amount authorized.
    #[napi(getter)]
    pub fn total_authorized(&self) -> BigInt {
        BigInt::from(
            self.cumulative_amount
                .load(std::sync::atomic::Ordering::SeqCst),
        )
    }

    /// Gracefully shut down the client.
    ///
    /// For HTTP clients, this is a no-op since there are no persistent connections.
    #[napi]
    pub async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

// Note: Tests for napi bindings require Node.js runtime.
// The underlying crypto functionality is tested in opencapsule_node::crypto.
#[cfg(test)]
mod tests {
    use opencapsule_node::crypto::{CryptoProvider, DefaultCryptoProvider, EncryptionDirection};

    fn create_test_keypair(secret_bytes: [u8; 32]) -> ([u8; 32], [u8; 32]) {
        use ed25519_dalek::SigningKey;
        let signing_key = SigningKey::from_bytes(&secret_bytes);
        let public = signing_key.verifying_key().to_bytes();
        (secret_bytes, public)
    }

    #[test]
    fn test_underlying_crypto_derive_channel_keys() {
        let provider = DefaultCryptoProvider;

        let (user_secret, _user_public) = create_test_keypair([1u8; 32]);
        let (worker_secret, worker_public) = create_test_keypair([2u8; 32]);
        let (_, user_public) = create_test_keypair([1u8; 32]);
        let channel_id = [3u8; 32];

        // User derives keys
        let user_keys = provider
            .derive_channel_keys(&user_secret, &worker_public, &channel_id)
            .expect("key derivation should succeed");

        // Worker derives keys
        let worker_keys = provider
            .derive_channel_keys(&worker_secret, &user_public, &channel_id)
            .expect("key derivation should succeed");

        // Both should have same master key
        assert_eq!(user_keys.master_key(), worker_keys.master_key());
    }

    #[test]
    fn test_underlying_crypto_encrypt_decrypt_roundtrip() {
        let provider = DefaultCryptoProvider;

        let (user_secret, _) = create_test_keypair([1u8; 32]);
        let (worker_secret, worker_public) = create_test_keypair([2u8; 32]);
        let (_, user_public) = create_test_keypair([1u8; 32]);
        let channel_id = [3u8; 32];

        let user_keys = provider
            .derive_channel_keys(&user_secret, &worker_public, &channel_id)
            .unwrap();

        let worker_keys = provider
            .derive_channel_keys(&worker_secret, &user_public, &channel_id)
            .unwrap();

        let plaintext = b"Hello, OpenCapsule!";
        let job_id = "test-job-123";

        // User encrypts
        let encrypted = provider
            .encrypt_job_blob(plaintext, &user_keys, job_id, EncryptionDirection::Input)
            .unwrap();

        // Worker decrypts
        let decrypted = provider
            .decrypt_job_blob(&encrypted, &worker_keys, job_id, EncryptionDirection::Input)
            .unwrap();

        assert_eq!(decrypted, plaintext);
    }
}
