#![deny(clippy::all)]

//! Native bindings for the Graphene Network SDK.
//!
//! This crate provides Node.js bindings via napi-rs for:
//! - Channel key derivation from Ed25519 identities
//! - Job encryption/decryption with forward secrecy
//! - Payment ticket signing and verification

use napi::bindgen_prelude::*;
use napi_derive::napi;

use monad_node::crypto::{
    self, CryptoProvider, DefaultCryptoProvider, EncryptedBlob as RustEncryptedBlob,
    EncryptionDirection as RustEncryptionDirection,
};
use monad_node::ticket::{
    ChannelState as RustChannelState, DefaultTicketValidator, PaymentTicket as RustPaymentTicket,
    TicketError, TicketPayload, TicketValidator,
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

    #[error("Ticket error: {0}")]
    Ticket(String),
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

impl From<TicketError> for SdkError {
    fn from(err: TicketError) -> Self {
        SdkError::Ticket(err.to_string())
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

/// Derive channel keys from Ed25519 identities and a Solana payment channel PDA.
///
/// This performs:
/// 1. Ed25519 -> X25519 key conversion
/// 2. X25519 ECDH between local secret and peer public
/// 3. HKDF with channel PDA as salt to derive the channel master key
///
/// Both parties in a payment channel will derive the same master key.
///
/// # Arguments
/// * `local_secret` - Your Ed25519 secret key (32 bytes)
/// * `peer_pubkey` - Peer's Ed25519 public key (32 bytes)
/// * `channel_pda` - Solana PDA for the payment channel (32 bytes)
///
/// # Returns
/// ChannelKeys containing the shared master key and X25519 keys
#[napi]
pub fn derive_channel_keys(
    local_secret: Buffer,
    peer_pubkey: Buffer,
    channel_pda: Buffer,
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
    if channel_pda.len() != 32 {
        return Err(SdkError::InvalidKeyLength {
            expected: 32,
            actual: channel_pda.len(),
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
    let channel_pda_arr: [u8; 32] = channel_pda
        .as_ref()
        .try_into()
        .expect("length already validated");

    let provider = DefaultCryptoProvider;
    let inner = provider
        .derive_channel_keys(&local_secret_arr, &peer_pubkey_arr, &channel_pda_arr)
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
// Payment Ticket Bindings
// ============================================================================

/// A payment ticket authorizing off-chain job payments.
///
/// Contains:
/// - channel_id: Payment channel address (32 bytes)
/// - amount_micros: Cumulative amount authorized in microtokens
/// - nonce: Monotonically increasing sequence number
/// - timestamp: Unix epoch seconds when ticket was created
/// - signature: Ed25519 signature over the 48-byte payload
#[napi]
pub struct PaymentTicket {
    inner: RustPaymentTicket,
}

#[napi]
impl PaymentTicket {
    /// Get the payment channel ID (32 bytes).
    #[napi(getter)]
    pub fn channel_id(&self) -> Buffer {
        Buffer::from(self.inner.channel_id.to_vec())
    }

    /// Get the cumulative amount authorized in microtokens.
    #[napi(getter)]
    pub fn amount_micros(&self) -> BigInt {
        BigInt::from(self.inner.amount_micros)
    }

    /// Get the ticket nonce (sequence number).
    #[napi(getter)]
    pub fn nonce(&self) -> BigInt {
        BigInt::from(self.inner.nonce)
    }

    /// Get the ticket timestamp (Unix epoch seconds).
    #[napi(getter)]
    pub fn timestamp(&self) -> i64 {
        self.inner.timestamp
    }

    /// Get the Ed25519 signature (64 bytes).
    #[napi]
    pub fn signature(&self) -> Buffer {
        Buffer::from(self.inner.signature().to_vec())
    }

    /// Serialize the ticket to bytes for transmission/storage.
    #[napi]
    pub fn to_bytes(&self) -> Result<Buffer> {
        let bytes = bincode::serialize(&self.inner)
            .map_err(|e| napi::Error::from_reason(format!("Serialization failed: {}", e)))?;
        Ok(Buffer::from(bytes))
    }

    /// Deserialize a ticket from bytes.
    #[napi(factory)]
    pub fn from_bytes(bytes: Buffer) -> Result<PaymentTicket> {
        let inner: RustPaymentTicket = bincode::deserialize(&bytes)
            .map_err(|e| napi::Error::from_reason(format!("Deserialization failed: {}", e)))?;
        Ok(PaymentTicket { inner })
    }
}

/// Channel state for ticket validation context.
///
/// Workers track this state per-channel to validate incoming tickets.
#[napi(object)]
pub struct ChannelState {
    /// Last seen nonce for this channel.
    pub last_nonce: BigInt,
    /// Last cumulative amount seen.
    pub last_amount: BigInt,
    /// Total balance available in the channel.
    pub channel_balance: BigInt,
}

/// Create a new payment ticket with the given parameters.
///
/// Signs the 48-byte payload (channel_id || amount_micros || nonce) with Ed25519.
/// The timestamp is set to the current time.
///
/// # Arguments
/// * `channel_id` - Payment channel address (32 bytes)
/// * `amount_micros` - Cumulative amount to authorize (u64 as BigInt)
/// * `nonce` - Ticket sequence number (u64 as BigInt)
/// * `signer_secret` - Ed25519 secret key (32 bytes)
///
/// # Returns
/// A signed PaymentTicket ready for transmission to workers.
#[napi]
pub fn create_payment_ticket(
    channel_id: Buffer,
    amount_micros: BigInt,
    nonce: BigInt,
    signer_secret: Buffer,
) -> Result<PaymentTicket> {
    use ed25519_dalek::{Signer, SigningKey};
    use std::time::{SystemTime, UNIX_EPOCH};

    // Validate input lengths
    if channel_id.len() != 32 {
        return Err(SdkError::InvalidKeyLength {
            expected: 32,
            actual: channel_id.len(),
        }
        .into());
    }
    if signer_secret.len() != 32 {
        return Err(SdkError::InvalidKeyLength {
            expected: 32,
            actual: signer_secret.len(),
        }
        .into());
    }

    // Convert BigInt to u64
    let (_, amount_bytes, _) = amount_micros.get_u64();
    let (_, nonce_bytes, _) = nonce.get_u64();

    // Convert to fixed arrays
    let channel_id_arr: [u8; 32] = channel_id
        .as_ref()
        .try_into()
        .expect("length already validated");
    let signer_secret_arr: [u8; 32] = signer_secret
        .as_ref()
        .try_into()
        .expect("length already validated");

    // Create the payload and sign it
    let payload = TicketPayload {
        channel_id: channel_id_arr,
        amount_micros: amount_bytes,
        nonce: nonce_bytes,
    };

    let signing_key = SigningKey::from_bytes(&signer_secret_arr);
    let message = payload.to_bytes();
    let signature = signing_key.sign(&message);

    // Get current timestamp
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| SdkError::Ticket("Failed to get system time".to_string()))?
        .as_secs() as i64;

    let inner = RustPaymentTicket::new(
        channel_id_arr,
        amount_bytes,
        nonce_bytes,
        timestamp,
        signature.to_bytes(),
    );

    Ok(PaymentTicket { inner })
}

/// Verify the Ed25519 signature on a payment ticket.
///
/// This only checks the cryptographic signature, not business rules like
/// nonce ordering or balance limits.
///
/// # Arguments
/// * `ticket` - The ticket to verify
/// * `payer_pubkey` - Expected Ed25519 public key of the payer (32 bytes)
///
/// # Returns
/// `true` if signature is valid, `false` otherwise
#[napi]
pub fn verify_ticket_signature(ticket: &PaymentTicket, payer_pubkey: Buffer) -> Result<bool> {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    // Validate input length
    if payer_pubkey.len() != 32 {
        return Err(SdkError::InvalidKeyLength {
            expected: 32,
            actual: payer_pubkey.len(),
        }
        .into());
    }

    let pubkey_arr: [u8; 32] = payer_pubkey
        .as_ref()
        .try_into()
        .expect("length already validated");

    // Parse the public key
    let verifying_key = match VerifyingKey::from_bytes(&pubkey_arr) {
        Ok(k) => k,
        Err(_) => return Ok(false),
    };

    // Parse the signature
    let signature = Signature::from_bytes(ticket.inner.signature());

    // Verify
    let message = ticket.inner.signed_message();
    Ok(verifying_key.verify(&message, &signature).is_ok())
}

/// Validate a payment ticket against channel state.
///
/// Performs full validation including:
/// 1. Ed25519 signature verification
/// 2. Nonce must be strictly greater than last_nonce (replay protection)
/// 3. Amount must be >= last_amount (cumulative)
/// 4. Amount must be <= channel_balance
/// 5. Timestamp must be within acceptable bounds (not too old or too far in future)
///
/// # Arguments
/// * `ticket` - The ticket to validate
/// * `payer_pubkey` - Expected Ed25519 public key of the payer (32 bytes)
/// * `channel_state` - Current state of the payment channel
///
/// # Returns
/// `Ok(())` if valid, throws an error with details on failure.
#[napi]
pub async fn validate_ticket(
    ticket: &PaymentTicket,
    payer_pubkey: Buffer,
    channel_state: ChannelState,
) -> Result<()> {
    // Validate input length
    if payer_pubkey.len() != 32 {
        return Err(SdkError::InvalidKeyLength {
            expected: 32,
            actual: payer_pubkey.len(),
        }
        .into());
    }

    let pubkey_arr: [u8; 32] = payer_pubkey
        .as_ref()
        .try_into()
        .expect("length already validated");

    // Convert ChannelState from napi object to Rust struct
    let (_, last_nonce, _) = channel_state.last_nonce.get_u64();
    let (_, last_amount, _) = channel_state.last_amount.get_u64();
    let (_, channel_balance, _) = channel_state.channel_balance.get_u64();

    let rust_channel_state = RustChannelState {
        last_nonce,
        last_amount,
        channel_balance,
    };

    // Use the default validator
    let validator = DefaultTicketValidator::new();
    validator
        .validate(&ticket.inner, &pubkey_arr, &rust_channel_state)
        .await
        .map_err(SdkError::from)?;

    Ok(())
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
// Protocol Bindings (JobRequest/JobResponse serialization)
// ============================================================================

use monad_node::p2p::messages::{
    EgressRule as RustEgressRule, JobManifest as RustJobManifest,
    ResultDeliveryMode as RustResultDeliveryMode,
};
use monad_node::p2p::protocol::{
    wire::{decode_message as wire_decode, encode_message as wire_encode, MessageType},
    AssetData, Compression, JobAssets as RustJobAssets, JobRequest as RustJobRequest,
    JobResponse as RustJobResponse, JobStatus as RustJobStatus, RejectReason as RustRejectReason,
    INLINE_CODE_THRESHOLD, INLINE_INPUT_THRESHOLD, MAX_MESSAGE_SIZE,
};
use std::collections::HashMap;

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

/// Resource requirements and configuration for a job.
#[napi(object)]
pub struct JobManifest {
    /// Required vCPUs.
    pub vcpu: u32,
    /// Required memory in MB.
    pub memory_mb: u32,
    /// Maximum execution time in milliseconds.
    pub timeout_ms: BigInt,
    /// Required unikernel image (e.g., "python:3.12").
    pub kernel: String,
    /// Allowed egress endpoints.
    pub egress_allowlist: Vec<EgressRule>,
    /// Environment variables to set in the unikernel.
    pub env: HashMap<String, String>,
    /// Estimated network egress in megabytes (optional).
    pub estimated_egress_mb: Option<BigInt>,
    /// Estimated network ingress in megabytes (optional).
    pub estimated_ingress_mb: Option<BigInt>,
}

/// References to code and input blobs in Iroh.
#[napi(object)]
pub struct JobAssets {
    /// BLAKE3 hash of the encrypted code blob (32 bytes).
    pub code_hash: Buffer,
    /// Optional URL to fetch code from.
    pub code_url: Option<String>,
    /// BLAKE3 hash of the encrypted input blob (32 bytes).
    pub input_hash: Buffer,
    /// Optional URL to fetch input from.
    pub input_url: Option<String>,
}

/// Job submission request from client to worker.
#[napi(object)]
pub struct JobRequest {
    /// Unique job identifier (UUID string).
    pub job_id: String,
    /// Resource requirements and configuration.
    pub manifest: JobManifest,
    /// Payment authorization ticket (serialized bytes).
    pub ticket: Buffer,
    /// Code and input blob references.
    pub assets: JobAssets,
    /// Ephemeral X25519 public key for forward secrecy (32 bytes).
    pub ephemeral_pubkey: Buffer,
    /// Solana PDA of the payment channel (32 bytes).
    pub channel_pda: Buffer,
    /// Requested result delivery mode ("sync" | "async").
    pub delivery_mode: String,
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
    /// Total cost charged in microtokens.
    pub total_cost_micros: BigInt,
    /// CPU cost component in microtokens.
    pub cpu_cost_micros: BigInt,
    /// Memory cost component in microtokens.
    pub memory_cost_micros: BigInt,
    /// Egress cost component in microtokens.
    pub egress_cost_micros: BigInt,
}

/// Job execution result.
#[napi(object)]
pub struct JobResult {
    /// BLAKE3 hash of the encrypted result blob (32 bytes).
    pub result_hash: Buffer,
    /// Optional URL to fetch result from.
    pub result_url: Option<String>,
    /// Exit code of the unikernel (0 = success).
    pub exit_code: i32,
    /// Execution duration in milliseconds.
    pub duration_ms: BigInt,
    /// Resource usage metrics.
    pub metrics: JobMetrics,
    /// Worker's Ed25519 signature (64 bytes).
    pub worker_signature: Buffer,
}

/// Status of a job in the protocol.
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
    /// Payment ticket signature or format is invalid.
    TicketInvalid,
    /// Payment channel balance exhausted or nonce replayed.
    ChannelExhausted,
    /// Payment does not authorize enough funds.
    InsufficientPayment,
    /// Worker is at capacity.
    CapacityFull,
    /// Requested kernel is not supported.
    UnsupportedKernel,
    /// Requested resources exceed worker limits.
    ResourcesExceedLimits,
    /// Environment variables total size exceeds limit.
    EnvTooLarge,
    /// Environment variable name is invalid.
    InvalidEnvName,
    /// Environment variable uses reserved GRAPHENE_* prefix.
    ReservedEnvPrefix,
    /// Code or input blob could not be fetched.
    AssetUnavailable,
    /// Inline asset exceeds maximum allowed size.
    InlineTooLarge,
    /// Generic internal error.
    InternalError,
}

/// Response to a job request.
#[napi(object)]
pub struct JobResponse {
    /// The job ID this response refers to.
    pub job_id: String,
    /// Current status of the job.
    pub status: String,
    /// Job result (only present when status is Succeeded, Failed, or Timeout).
    pub result: Option<JobResult>,
    /// Error message (only present when status is Rejected).
    pub error: Option<String>,
    /// Reject reason (only present when status is Rejected).
    pub reject_reason: Option<String>,
}

/// A decoded wire message containing type and payload.
#[napi(object)]
pub struct WireMessage {
    /// Message type byte.
    pub msg_type: u32,
    /// Raw payload bytes (bincode-encoded).
    pub payload: Buffer,
}

/// Serialize a JobRequest to wire format bytes.
///
/// The wire format is: [4 bytes: length BE] [1 byte: type] [N bytes: bincode payload]
///
/// # Arguments
/// * `request` - The job request to serialize
///
/// # Returns
/// Wire-formatted bytes ready for transmission
#[napi]
pub fn serialize_job_request(request: JobRequest) -> Result<Buffer> {
    use iroh_blobs::Hash;
    use uuid::Uuid;

    // Parse job_id as UUID
    let job_id = Uuid::parse_str(&request.job_id)
        .map_err(|e| napi::Error::from_reason(format!("Invalid job_id UUID: {}", e)))?;

    // Convert manifest
    let egress_allowlist: Vec<RustEgressRule> = request
        .manifest
        .egress_allowlist
        .into_iter()
        .map(|r| RustEgressRule {
            host: r.host,
            port: r.port as u16,
            protocol: r.protocol,
        })
        .collect();

    let (_, timeout_ms, _) = request.manifest.timeout_ms.get_u64();
    let estimated_egress_mb = request.manifest.estimated_egress_mb.map(|b| b.get_u64().1);
    let estimated_ingress_mb = request.manifest.estimated_ingress_mb.map(|b| b.get_u64().1);

    let manifest = RustJobManifest {
        vcpu: request.manifest.vcpu as u8,
        memory_mb: request.manifest.memory_mb,
        timeout_ms,
        kernel: request.manifest.kernel,
        egress_allowlist,
        env: request.manifest.env,
        estimated_egress_mb,
        estimated_ingress_mb,
    };

    // Deserialize the ticket from bytes
    let ticket: RustPaymentTicket = bincode::deserialize(&request.ticket)
        .map_err(|e| napi::Error::from_reason(format!("Invalid ticket bytes: {}", e)))?;

    // Convert assets
    if request.assets.code_hash.len() != 32 {
        return Err(napi::Error::from_reason("code_hash must be 32 bytes"));
    }
    if request.assets.input_hash.len() != 32 {
        return Err(napi::Error::from_reason("input_hash must be 32 bytes"));
    }

    let code_hash_arr: [u8; 32] = request.assets.code_hash.as_ref().try_into().unwrap();
    let input_hash_arr: [u8; 32] = request.assets.input_hash.as_ref().try_into().unwrap();

    // Build JobAssets using the new blob-based constructor
    let input_hash = if input_hash_arr.iter().all(|&b| b == 0) {
        None
    } else {
        Some(Hash::from_bytes(input_hash_arr))
    };
    let assets = RustJobAssets::from_blobs(
        Hash::from_bytes(code_hash_arr),
        request.assets.code_url,
        input_hash.unwrap_or_else(|| Hash::from_bytes([0u8; 32])),
        request.assets.input_url,
    );

    // Convert ephemeral_pubkey and channel_pda
    if request.ephemeral_pubkey.len() != 32 {
        return Err(napi::Error::from_reason(
            "ephemeral_pubkey must be 32 bytes",
        ));
    }
    if request.channel_pda.len() != 32 {
        return Err(napi::Error::from_reason("channel_pda must be 32 bytes"));
    }

    let ephemeral_pubkey: [u8; 32] = request.ephemeral_pubkey.as_ref().try_into().unwrap();
    let channel_pda: [u8; 32] = request.channel_pda.as_ref().try_into().unwrap();

    // Convert delivery mode
    let delivery_mode = match request.delivery_mode.to_lowercase().as_str() {
        "async" => RustResultDeliveryMode::Async,
        _ => RustResultDeliveryMode::Sync,
    };

    let rust_request = RustJobRequest {
        job_id,
        manifest,
        ticket,
        assets,
        ephemeral_pubkey,
        channel_pda,
        delivery_mode,
    };

    let bytes = wire_encode(MessageType::JobRequest, &rust_request)
        .map_err(|e| napi::Error::from_reason(format!("Serialization failed: {}", e)))?;

    Ok(Buffer::from(bytes))
}

/// Deserialize a JobResponse from wire format bytes.
///
/// # Arguments
/// * `data` - Wire-formatted bytes received from a worker
///
/// # Returns
/// Parsed JobResponse
#[napi]
pub fn deserialize_job_response(data: Buffer) -> Result<JobResponse> {
    let (msg_type, payload) = wire_decode(&data)
        .map_err(|e| napi::Error::from_reason(format!("Wire decode failed: {}", e)))?;

    // Accept JobAccepted (0x02), JobResult (0x04), or JobRejected (0x05)
    match msg_type {
        MessageType::JobAccepted | MessageType::JobResult | MessageType::JobRejected => {}
        _ => {
            return Err(napi::Error::from_reason(format!(
                "Unexpected message type: {:?}",
                msg_type
            )));
        }
    }

    let rust_response: RustJobResponse = bincode::deserialize(payload)
        .map_err(|e| napi::Error::from_reason(format!("Payload decode failed: {}", e)))?;

    // Convert status and extract reject reason if present
    let (status, reject_reason) = match rust_response.status {
        RustJobStatus::Accepted => ("Accepted".to_string(), None),
        RustJobStatus::Running => ("Running".to_string(), None),
        RustJobStatus::Succeeded => ("Succeeded".to_string(), None),
        RustJobStatus::Failed => ("Failed".to_string(), None),
        RustJobStatus::Timeout => ("Timeout".to_string(), None),
        RustJobStatus::Rejected(reason) => {
            let reason_str = match reason {
                RustRejectReason::TicketInvalid => "TicketInvalid",
                RustRejectReason::ChannelExhausted => "ChannelExhausted",
                RustRejectReason::InsufficientPayment => "InsufficientPayment",
                RustRejectReason::CapacityFull => "CapacityFull",
                RustRejectReason::UnsupportedKernel => "UnsupportedKernel",
                RustRejectReason::ResourcesExceedLimits => "ResourcesExceedLimits",
                RustRejectReason::EnvTooLarge => "EnvTooLarge",
                RustRejectReason::InvalidEnvName => "InvalidEnvName",
                RustRejectReason::ReservedEnvPrefix => "ReservedEnvPrefix",
                RustRejectReason::AssetUnavailable => "AssetUnavailable",
                RustRejectReason::InlineTooLarge => "InlineTooLarge",
                RustRejectReason::InternalError => "InternalError",
            };
            ("Rejected".to_string(), Some(reason_str.to_string()))
        }
    };

    // Convert result if present
    let result = rust_response.result.map(|r| JobResult {
        result_hash: Buffer::from(r.result_hash.as_bytes().to_vec()),
        result_url: r.result_url,
        exit_code: r.exit_code,
        duration_ms: BigInt::from(r.duration_ms),
        metrics: JobMetrics {
            peak_memory_bytes: BigInt::from(r.metrics.peak_memory_bytes),
            cpu_time_ms: BigInt::from(r.metrics.cpu_time_ms),
            network_rx_bytes: BigInt::from(r.metrics.network_rx_bytes),
            network_tx_bytes: BigInt::from(r.metrics.network_tx_bytes),
            total_cost_micros: BigInt::from(r.metrics.total_cost_micros),
            cpu_cost_micros: BigInt::from(r.metrics.cpu_cost_micros),
            memory_cost_micros: BigInt::from(r.metrics.memory_cost_micros),
            egress_cost_micros: BigInt::from(r.metrics.egress_cost_micros),
        },
        worker_signature: Buffer::from(r.worker_signature.as_bytes().to_vec()),
    });

    Ok(JobResponse {
        job_id: rust_response.job_id.to_string(),
        status,
        result,
        error: rust_response.error,
        reject_reason,
    })
}

/// Encode a raw payload into wire format with the given message type.
///
/// Wire format: [4 bytes: length BE] [1 byte: type] [payload]
///
/// # Arguments
/// * `msg_type` - Message type byte (1=JobRequest, 2=JobAccepted, 3=JobProgress, 4=JobResult, 5=JobRejected)
/// * `payload` - Raw payload bytes
///
/// # Returns
/// Wire-formatted bytes
#[napi]
pub fn encode_wire_message(msg_type: u32, payload: Buffer) -> Result<Buffer> {
    // Validate message type
    if !(1..=5).contains(&msg_type) {
        return Err(napi::Error::from_reason(format!(
            "Invalid message type: {}. Must be 1-5.",
            msg_type
        )));
    }

    // Calculate length: 1 byte type + payload length
    let content_len = 1 + payload.len();
    if content_len > 16 * 1024 * 1024 {
        return Err(napi::Error::from_reason("Message too large (max 16MB)"));
    }

    let mut buf = Vec::with_capacity(4 + content_len);
    buf.extend_from_slice(&(content_len as u32).to_be_bytes());
    buf.push(msg_type as u8);
    buf.extend_from_slice(&payload);

    Ok(Buffer::from(buf))
}

/// Decode a wire message into its type and payload.
///
/// # Arguments
/// * `data` - Wire-formatted bytes
///
/// # Returns
/// WireMessage containing the message type and raw payload
#[napi]
pub fn decode_wire_message(data: Buffer) -> Result<WireMessage> {
    // Need at least 5 bytes: 4 (length) + 1 (type)
    if data.len() < 5 {
        return Err(napi::Error::from_reason(format!(
            "Message truncated: expected at least 5 bytes, got {}",
            data.len()
        )));
    }

    let length = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;

    if length > 16 * 1024 * 1024 {
        return Err(napi::Error::from_reason(format!(
            "Message too large: {} bytes (max 16MB)",
            length
        )));
    }

    // Check we have enough data
    if data.len() < 4 + length {
        return Err(napi::Error::from_reason(format!(
            "Message truncated: expected {} bytes, got {}",
            4 + length,
            data.len()
        )));
    }

    let msg_type = data[4];
    let payload = data[5..4 + length].to_vec();

    Ok(WireMessage {
        msg_type: msg_type as u32,
        payload: Buffer::from(payload),
    })
}

// ============================================================================
// High-Level Client API
// ============================================================================

use monad_node::p2p::protocol::wire::{try_read_message, MessageType as WireMessageType};
use monad_node::p2p::{graphene::GrapheneNode, P2PConfig, P2PNetwork, RelayConfig};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Configuration for creating a Graphene client.
#[napi(object)]
pub struct ClientConfig {
    /// Storage path for persistent data (identity key, blob cache).
    pub storage_path: String,
    /// Your Ed25519 secret key (32 bytes).
    pub secret_key: Buffer,
    /// Payment channel PDA (32 bytes).
    pub channel_pda: Buffer,
    /// Worker's node ID - hex-encoded Ed25519 public key (64 hex chars).
    /// This is used for both P2P connection and encryption key derivation.
    pub worker_node_id: String,
    /// Whether to use relay servers for NAT traversal.
    pub use_relay: Option<bool>,
    /// Optional bind port (0 for random).
    pub bind_port: Option<u32>,
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
    /// Delivery mode: "auto", "inline", or "blob".
    /// - "auto" (default): Use inline for small payloads, blob for large.
    /// - "inline": Always inline, error if over 16 MB message limit.
    /// - "blob": Always upload to Iroh blob storage.
    pub mode: Option<String>,
    /// Threshold for inline code in bytes (default: 4MB, only for "auto" mode).
    pub inline_code_threshold: Option<u32>,
    /// Threshold for inline input in bytes (default: 8MB, only for "auto" mode).
    pub inline_input_threshold: Option<u32>,
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
    /// Asset delivery options (mode, compression, thresholds).
    pub assets: Option<AssetOptions>,
    /// Timeout in milliseconds (default: 30000).
    pub timeout_ms: Option<BigInt>,
    /// Kernel/runtime to use (default: "python:3.12").
    pub kernel: Option<String>,
    /// Environment variables.
    pub env: Option<HashMap<String, String>>,
    /// Result delivery mode: "sync" or "async".
    pub delivery_mode: Option<String>,
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

/// A native Graphene network client.
///
/// Handles everything internally:
/// - Channel key derivation
/// - Job encryption/decryption
/// - Payment ticket creation
/// - Blob upload/download
/// - Protocol serialization
/// - Network transport
#[napi]
pub struct GrapheneClient {
    node: Arc<RwLock<Option<GrapheneNode>>>,
    channel_keys: crypto::ChannelKeys,
    secret_key: [u8; 32],
    channel_pda: [u8; 32],
    worker_node_id: String,
    nonce: AtomicU64,
    cumulative_amount: AtomicU64,
}

#[napi]
impl GrapheneClient {
    /// Create a new Graphene client.
    ///
    /// This initializes:
    /// - Channel key derivation for end-to-end encryption
    /// - P2P networking (QUIC endpoint with NAT traversal)
    /// - Blob storage for code/input/output transfers
    ///
    /// # Arguments
    /// * `config` - Client configuration with keys and worker info
    #[napi(factory)]
    pub async fn create(config: ClientConfig) -> Result<GrapheneClient> {
        // Validate key lengths
        if config.secret_key.len() != 32 {
            return Err(napi::Error::from_reason(format!(
                "secret_key must be 32 bytes, got {}",
                config.secret_key.len()
            )));
        }
        if config.channel_pda.len() != 32 {
            return Err(napi::Error::from_reason(format!(
                "channel_pda must be 32 bytes, got {}",
                config.channel_pda.len()
            )));
        }

        // Parse worker_node_id (hex-encoded Ed25519 public key)
        let worker_pubkey: [u8; 32] = hex::decode(&config.worker_node_id)
            .map_err(|e| napi::Error::from_reason(format!("Invalid worker_node_id hex: {}", e)))?
            .try_into()
            .map_err(|_| {
                napi::Error::from_reason(
                    "worker_node_id must be 64 hex characters (32 bytes)".to_string(),
                )
            })?;

        // Convert to fixed arrays
        let secret_key: [u8; 32] = config.secret_key.as_ref().try_into().unwrap();
        let channel_pda: [u8; 32] = config.channel_pda.as_ref().try_into().unwrap();

        // Derive channel keys
        let provider = DefaultCryptoProvider;
        let channel_keys = provider
            .derive_channel_keys(&secret_key, &worker_pubkey, &channel_pda)
            .map_err(|e| {
                napi::Error::from_reason(format!("Failed to derive channel keys: {}", e))
            })?;

        // Initialize P2P node
        let p2p_config = P2PConfig {
            storage_path: config.storage_path.into(),
            relay_config: if config.use_relay.unwrap_or(true) {
                RelayConfig::Default
            } else {
                RelayConfig::Disabled
            },
            bootstrap_peers: Vec::new(),
            bind_port: config.bind_port.unwrap_or(0) as u16,
        };

        let node = GrapheneNode::new(p2p_config)
            .await
            .map_err(|e| napi::Error::from_reason(format!("Failed to create P2P node: {}", e)))?;

        Ok(GrapheneClient {
            node: Arc::new(RwLock::new(Some(node))),
            channel_keys,
            secret_key,
            channel_pda,
            worker_node_id: config.worker_node_id,
            nonce: AtomicU64::new(0),
            cumulative_amount: AtomicU64::new(0),
        })
    }

    /// Get this client's node ID (public key) as a hex string.
    #[napi]
    pub async fn node_id(&self) -> Result<String> {
        let guard = self.node.read().await;
        let node = guard
            .as_ref()
            .ok_or_else(|| napi::Error::from_reason("Client has been shut down"))?;
        Ok(node.node_id().to_string())
    }

    /// Upload a blob and return its BLAKE3 hash.
    ///
    /// # Arguments
    /// * `data` - The data to upload
    ///
    /// # Returns
    /// The BLAKE3 hash of the uploaded blob (32 bytes)
    #[napi]
    pub async fn upload_blob(&self, data: Buffer) -> Result<Buffer> {
        let guard = self.node.read().await;
        let node = guard
            .as_ref()
            .ok_or_else(|| napi::Error::from_reason("Client has been shut down"))?;

        let hash = node
            .upload_blob(&data)
            .await
            .map_err(|e| napi::Error::from_reason(format!("Upload failed: {}", e)))?;

        Ok(Buffer::from(hash.as_bytes().to_vec()))
    }

    /// Download a blob by its BLAKE3 hash.
    ///
    /// # Arguments
    /// * `hash` - The BLAKE3 hash of the blob (32 bytes)
    /// * `from_node_id` - Optional peer node ID (hex string) to download from
    ///
    /// # Returns
    /// The blob data
    #[napi]
    pub async fn download_blob(
        &self,
        hash: Buffer,
        from_node_id: Option<String>,
    ) -> Result<Buffer> {
        let guard = self.node.read().await;
        let node = guard
            .as_ref()
            .ok_or_else(|| napi::Error::from_reason("Client has been shut down"))?;

        if hash.len() != 32 {
            return Err(napi::Error::from_reason("Hash must be 32 bytes"));
        }

        let hash_arr: [u8; 32] = hash.as_ref().try_into().unwrap();
        let iroh_hash = iroh_blobs::Hash::from_bytes(hash_arr);

        let from = match from_node_id {
            Some(node_id_str) => {
                let pubkey: iroh::PublicKey = node_id_str
                    .parse()
                    .map_err(|e| napi::Error::from_reason(format!("Invalid node ID: {}", e)))?;
                Some(iroh::EndpointAddr::new(pubkey))
            }
            None => None,
        };

        let data = node
            .download_blob(iroh_hash, from)
            .await
            .map_err(|e| napi::Error::from_reason(format!("Download failed: {}", e)))?;

        Ok(Buffer::from(data))
    }

    /// Send a job request to a worker and receive the response.
    ///
    /// This establishes a QUIC connection to the worker, sends the serialized
    /// job request, and waits for the response.
    ///
    /// # Arguments
    /// * `worker_node_id` - The worker's node ID (hex string)
    /// * `request` - Wire-formatted job request bytes
    ///
    /// # Returns
    /// Wire-formatted job response bytes
    #[napi]
    pub async fn send_job_request(
        &self,
        worker_node_id: String,
        request: Buffer,
    ) -> Result<Buffer> {
        let guard = self.node.read().await;
        let node = guard
            .as_ref()
            .ok_or_else(|| napi::Error::from_reason("Client has been shut down"))?;

        // Parse worker node ID and create address
        let pubkey: iroh::PublicKey = worker_node_id
            .parse()
            .map_err(|e| napi::Error::from_reason(format!("Invalid worker node ID: {}", e)))?;
        let addr = iroh::EndpointAddr::new(pubkey);

        // Connect to worker using job protocol ALPN
        let conn = node
            .connect(addr, monad_node::p2p::graphene::GRAPHENE_JOB_ALPN)
            .await
            .map_err(|e| napi::Error::from_reason(format!("Connection failed: {}", e)))?;

        // Open bidirectional stream
        let (mut send, mut recv) = conn
            .open_bi()
            .await
            .map_err(|e| napi::Error::from_reason(format!("Failed to open stream: {}", e)))?;

        // Send the request
        send.write_all(&request)
            .await
            .map_err(|e| napi::Error::from_reason(format!("Write failed: {}", e)))?;
        send.finish()
            .map_err(|e| napi::Error::from_reason(format!("Finish failed: {}", e)))?;

        // Read response
        let mut response_buf = Vec::with_capacity(64 * 1024);
        loop {
            let mut chunk = vec![0u8; 16 * 1024];
            match recv.read(&mut chunk).await {
                Ok(Some(0)) | Ok(None) => break,
                Ok(Some(n)) => {
                    response_buf.extend_from_slice(&chunk[..n]);

                    // Try to parse - check if we have a terminal message
                    if let Some((msg_type, _, consumed)) = try_read_message(&response_buf)
                        .map_err(|e| napi::Error::from_reason(format!("Wire error: {}", e)))?
                    {
                        match msg_type {
                            // Terminal messages - stop reading
                            WireMessageType::JobResult | WireMessageType::JobRejected => {
                                response_buf.truncate(consumed);
                                break;
                            }
                            // JobAccepted is an ack - continue reading for the result
                            WireMessageType::JobAccepted => {
                                // Remove the accepted message from buffer and continue
                                response_buf.drain(..consumed);
                            }
                            // Progress messages etc - continue reading
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    return Err(napi::Error::from_reason(format!("Read failed: {}", e)));
                }
            }
        }

        if response_buf.is_empty() {
            return Err(napi::Error::from_reason("No response received from worker"));
        }

        Ok(Buffer::from(response_buf))
    }

    /// Submit a job to the worker.
    ///
    /// This handles everything internally:
    /// 1. Generates unique job ID
    /// 2. Encrypts code and input
    /// 3. Creates payment ticket
    /// 4. Uploads blobs to worker
    /// 5. Sends job request
    /// 6. Receives and decrypts response
    ///
    /// # Arguments
    /// * `options` - Job configuration
    ///
    /// # Returns
    /// Job result with decrypted output
    #[napi]
    pub async fn submit_job(&self, options: JobOptions) -> Result<NativeJobResult> {
        use ed25519_dalek::{Signer, SigningKey};
        use std::sync::atomic::Ordering;
        use std::time::{SystemTime, UNIX_EPOCH};

        let guard = self.node.read().await;
        let node = guard
            .as_ref()
            .ok_or_else(|| napi::Error::from_reason("Client has been shut down"))?;

        // Generate job ID
        let job_id = uuid::Uuid::new_v4();
        let job_id_str = job_id.to_string();

        // Apply defaults from nested structures
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
        let kernel = options.kernel.unwrap_or_else(|| "python:3.12".to_string());
        let env = options.env.unwrap_or_default();
        let egress_allowlist = networking.egress_allowlist.unwrap_or_default();
        let estimated_egress_mb = networking.estimated_egress_mb.map(|b| b.get_u64().1);
        let estimated_ingress_mb = networking.estimated_ingress_mb.map(|b| b.get_u64().1);
        let delivery_mode = match options.delivery_mode.as_deref() {
            Some("async") => RustResultDeliveryMode::Async,
            _ => RustResultDeliveryMode::Sync,
        };

        // Parse asset options
        let asset_opts = options.assets.unwrap_or_default();
        let mode = asset_opts.mode.as_deref().unwrap_or("auto");
        let compress = asset_opts.compress.unwrap_or(false);
        let code_threshold = asset_opts
            .inline_code_threshold
            .map(|t| t as usize)
            .unwrap_or(INLINE_CODE_THRESHOLD);
        let input_threshold = asset_opts
            .inline_input_threshold
            .map(|t| t as usize)
            .unwrap_or(INLINE_INPUT_THRESHOLD);

        // Encrypt code
        let provider = DefaultCryptoProvider;
        let code_bytes = options.code.as_bytes();

        // Optionally compress before encryption
        let code_to_encrypt: std::borrow::Cow<[u8]> =
            if compress {
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
                &job_id_str,
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
                    &job_id_str,
                    RustEncryptionDirection::Input,
                )
                .map_err(|e| napi::Error::from_reason(format!("Failed to encrypt input: {}", e)))?;
            Some(encrypted.to_bytes())
        } else {
            None
        };

        // Determine asset delivery based on mode
        let code_asset = match mode {
            "inline" => {
                if encrypted_code_bytes.len() > MAX_MESSAGE_SIZE {
                    return Err(napi::Error::from_reason(format!(
                        "Code too large for inline mode: {} bytes (max {} bytes)",
                        encrypted_code_bytes.len(),
                        MAX_MESSAGE_SIZE
                    )));
                }
                AssetData::inline(encrypted_code_bytes.clone())
            }
            "blob" => {
                let hash = node.upload_blob(&encrypted_code_bytes).await.map_err(|e| {
                    napi::Error::from_reason(format!("Failed to upload code: {}", e))
                })?;
                AssetData::blob(hash, None)
            }
            _ => {
                // "auto" mode - inline if under threshold, blob otherwise
                if encrypted_code_bytes.len() <= code_threshold {
                    AssetData::inline(encrypted_code_bytes.clone())
                } else {
                    let hash = node.upload_blob(&encrypted_code_bytes).await.map_err(|e| {
                        napi::Error::from_reason(format!("Failed to upload code: {}", e))
                    })?;
                    AssetData::blob(hash, None)
                }
            }
        };

        let input_asset = if let Some(ref input_bytes) = encrypted_input_bytes {
            let asset = match mode {
                "inline" => {
                    if input_bytes.len() > MAX_MESSAGE_SIZE {
                        return Err(napi::Error::from_reason(format!(
                            "Input too large for inline mode: {} bytes (max {} bytes)",
                            input_bytes.len(),
                            MAX_MESSAGE_SIZE
                        )));
                    }
                    AssetData::inline(input_bytes.clone())
                }
                "blob" => {
                    let hash = node.upload_blob(input_bytes).await.map_err(|e| {
                        napi::Error::from_reason(format!("Failed to upload input: {}", e))
                    })?;
                    AssetData::blob(hash, None)
                }
                _ => {
                    // "auto" mode
                    if input_bytes.len() <= input_threshold {
                        AssetData::inline(input_bytes.clone())
                    } else {
                        let hash = node.upload_blob(input_bytes).await.map_err(|e| {
                            napi::Error::from_reason(format!("Failed to upload input: {}", e))
                        })?;
                        AssetData::blob(hash, None)
                    }
                }
            };
            Some(asset)
        } else {
            None
        };

        let compression = if compress {
            Compression::Zstd
        } else {
            Compression::None
        };

        // Estimate cost and create payment ticket
        let cost_per_vcpu_ms = 1u64;
        let cost_per_mb_ms = 1u64;
        let estimated_cost = (vcpu as u64 * timeout_ms * cost_per_vcpu_ms)
            + (memory_mb as u64 * timeout_ms * cost_per_mb_ms);

        let new_nonce = self.nonce.fetch_add(1, Ordering::SeqCst) + 1;
        let new_amount = self
            .cumulative_amount
            .fetch_add(estimated_cost, Ordering::SeqCst)
            + estimated_cost;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| napi::Error::from_reason("Failed to get system time"))?
            .as_secs() as i64;

        let payload = TicketPayload {
            channel_id: self.channel_pda,
            amount_micros: new_amount,
            nonce: new_nonce,
        };
        let signing_key = SigningKey::from_bytes(&self.secret_key);
        let signature = signing_key.sign(&payload.to_bytes());
        let ticket = RustPaymentTicket::new(
            self.channel_pda,
            new_amount,
            new_nonce,
            timestamp,
            signature.to_bytes(),
        );

        // Build egress rules
        let egress: Vec<RustEgressRule> = egress_allowlist
            .into_iter()
            .map(|r| RustEgressRule {
                host: r.host,
                port: r.port as u16,
                protocol: r.protocol,
            })
            .collect();

        // Build job request
        let manifest = RustJobManifest {
            vcpu,
            memory_mb,
            timeout_ms,
            kernel,
            egress_allowlist: egress,
            env,
            estimated_egress_mb,
            estimated_ingress_mb,
        };

        let assets = RustJobAssets {
            code: code_asset,
            input: input_asset,
            files: vec![],
            compression,
        };

        let request = RustJobRequest {
            job_id,
            manifest,
            ticket,
            assets,
            ephemeral_pubkey: encrypted_code.ephemeral_pubkey,
            channel_pda: self.channel_pda,
            delivery_mode,
        };

        // Serialize and send
        let request_bytes = wire_encode(MessageType::JobRequest, &request)
            .map_err(|e| napi::Error::from_reason(format!("Serialization failed: {}", e)))?;

        // Connect to worker
        let worker_pubkey: iroh::PublicKey = self
            .worker_node_id
            .parse()
            .map_err(|e| napi::Error::from_reason(format!("Invalid worker node ID: {}", e)))?;
        let addr = iroh::EndpointAddr::new(worker_pubkey);

        let conn = node
            .connect(addr.clone(), monad_node::p2p::graphene::GRAPHENE_JOB_ALPN)
            .await
            .map_err(|e| napi::Error::from_reason(format!("Connection failed: {}", e)))?;

        let (mut send_stream, mut recv_stream) = conn
            .open_bi()
            .await
            .map_err(|e| napi::Error::from_reason(format!("Failed to open stream: {}", e)))?;

        send_stream
            .write_all(&request_bytes)
            .await
            .map_err(|e| napi::Error::from_reason(format!("Write failed: {}", e)))?;
        send_stream
            .finish()
            .map_err(|e| napi::Error::from_reason(format!("Finish failed: {}", e)))?;

        // Read response
        let mut response_buf = Vec::with_capacity(64 * 1024);
        loop {
            let mut chunk = vec![0u8; 16 * 1024];
            match recv_stream.read(&mut chunk).await {
                Ok(Some(0)) | Ok(None) => break,
                Ok(Some(n)) => {
                    response_buf.extend_from_slice(&chunk[..n]);
                    // Try to parse - check if we have a terminal message
                    if let Some((msg_type, _, consumed)) = try_read_message(&response_buf)
                        .map_err(|e| napi::Error::from_reason(format!("Wire error: {}", e)))?
                    {
                        match msg_type {
                            // Terminal messages - stop reading
                            WireMessageType::JobResult | WireMessageType::JobRejected => {
                                response_buf.truncate(consumed);
                                break;
                            }
                            // JobAccepted is an ack - continue reading for the result
                            WireMessageType::JobAccepted => {
                                // Remove the accepted message from buffer and continue
                                response_buf.drain(..consumed);
                            }
                            // Progress messages etc - continue reading
                            _ => {}
                        }
                    }
                }
                Err(e) => return Err(napi::Error::from_reason(format!("Read failed: {}", e))),
            }
        }

        if response_buf.is_empty() {
            return Err(napi::Error::from_reason("No response received from worker"));
        }

        // Deserialize response
        let (_, payload) = wire_decode(&response_buf)
            .map_err(|e| napi::Error::from_reason(format!("Wire decode failed: {}", e)))?;
        let response: RustJobResponse = bincode::deserialize(payload)
            .map_err(|e| napi::Error::from_reason(format!("Payload decode failed: {}", e)))?;

        // Check for rejection
        if let RustJobStatus::Rejected(reason) = &response.status {
            return Err(napi::Error::from_reason(format!(
                "Job rejected: {:?}",
                reason
            )));
        }

        let result = response
            .result
            .ok_or_else(|| napi::Error::from_reason("No result in response"))?;

        // Download and decrypt output
        let encrypted_output = node
            .download_blob(result.result_hash, Some(addr.clone()))
            .await
            .map_err(|e| napi::Error::from_reason(format!("Failed to download output: {}", e)))?;

        let encrypted_blob = RustEncryptedBlob::from_bytes(&encrypted_output)
            .map_err(|e| napi::Error::from_reason(format!("Invalid encrypted output: {}", e)))?;

        let decrypted_output = provider
            .decrypt_job_blob(
                &encrypted_blob,
                &self.channel_keys,
                &job_id_str,
                RustEncryptionDirection::Output,
            )
            .map_err(|e| napi::Error::from_reason(format!("Failed to decrypt output: {}", e)))?;

        Ok(NativeJobResult {
            exit_code: result.exit_code,
            output: Buffer::from(decrypted_output),
            duration_ms: BigInt::from(result.duration_ms),
            metrics: JobMetrics {
                peak_memory_bytes: BigInt::from(result.metrics.peak_memory_bytes),
                cpu_time_ms: BigInt::from(result.metrics.cpu_time_ms),
                network_rx_bytes: BigInt::from(result.metrics.network_rx_bytes),
                network_tx_bytes: BigInt::from(result.metrics.network_tx_bytes),
                total_cost_micros: BigInt::from(result.metrics.total_cost_micros),
                cpu_cost_micros: BigInt::from(result.metrics.cpu_cost_micros),
                memory_cost_micros: BigInt::from(result.metrics.memory_cost_micros),
                egress_cost_micros: BigInt::from(result.metrics.egress_cost_micros),
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
    #[napi]
    pub async fn shutdown(&self) -> Result<()> {
        let mut guard = self.node.write().await;
        if let Some(node) = guard.take() {
            node.shutdown()
                .await
                .map_err(|e| napi::Error::from_reason(format!("Shutdown failed: {}", e)))?;
        }
        Ok(())
    }
}

// Note: Tests for napi bindings require Node.js runtime.
// The underlying crypto functionality is tested in monad_node::crypto.
// See crates/sdk/tests/ for Node.js integration tests.
#[cfg(test)]
mod tests {
    use monad_node::crypto::{CryptoProvider, DefaultCryptoProvider, EncryptionDirection};

    fn create_test_keypair(secret_bytes: [u8; 32]) -> ([u8; 32], [u8; 32]) {
        use ed25519_dalek::SigningKey;
        let signing_key = SigningKey::from_bytes(&secret_bytes);
        let public = signing_key.verifying_key().to_bytes();
        (secret_bytes, public)
    }

    #[test]
    fn test_underlying_crypto_derive_channel_keys() {
        // Test the underlying Rust crypto, not the napi wrappers
        let provider = DefaultCryptoProvider;

        let (user_secret, _user_public) = create_test_keypair([1u8; 32]);
        let (worker_secret, worker_public) = create_test_keypair([2u8; 32]);
        let (_, user_public) = create_test_keypair([1u8; 32]);
        let channel_pda = [3u8; 32];

        // User derives keys
        let user_keys = provider
            .derive_channel_keys(&user_secret, &worker_public, &channel_pda)
            .expect("key derivation should succeed");

        // Worker derives keys
        let worker_keys = provider
            .derive_channel_keys(&worker_secret, &user_public, &channel_pda)
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
        let channel_pda = [3u8; 32];

        let user_keys = provider
            .derive_channel_keys(&user_secret, &worker_public, &channel_pda)
            .unwrap();

        let worker_keys = provider
            .derive_channel_keys(&worker_secret, &user_public, &channel_pda)
            .unwrap();

        let plaintext = b"Hello, Graphene!";
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
