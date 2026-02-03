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
// Protocol Bindings (JobRequest/JobResponse serialization)
// ============================================================================

use monad_node::p2p::messages::{
    EgressRule as RustEgressRule, JobManifest as RustJobManifest,
    ResultDeliveryMode as RustResultDeliveryMode,
};
use monad_node::p2p::protocol::{
    wire::{decode_message as wire_decode, encode_message as wire_encode, MessageType},
    JobAssets as RustJobAssets, JobRequest as RustJobRequest, JobResponse as RustJobResponse,
    JobStatus as RustJobStatus, RejectReason as RustRejectReason,
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

    let manifest = RustJobManifest {
        vcpu: request.manifest.vcpu as u8,
        memory_mb: request.manifest.memory_mb,
        timeout_ms,
        kernel: request.manifest.kernel,
        egress_allowlist,
        env: request.manifest.env,
        estimated_egress_mb,
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

    let assets = RustJobAssets {
        code_hash: Hash::from_bytes(code_hash_arr),
        code_url: request.assets.code_url,
        input_hash: Hash::from_bytes(input_hash_arr),
        input_url: request.assets.input_url,
    };

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
