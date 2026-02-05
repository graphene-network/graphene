//! QUIC stream handler for the job submission protocol.
//!
//! This module provides the [`JobProtocolHandler`] which processes incoming
//! job requests on QUIC bi-directional streams.

use super::types::{
    JobMetrics, JobProgress, JobRequest, JobResponse, JobResult, JobStatus, ProgressKind,
    RejectReason,
};
use super::validation::{validate_env, EnvValidationError};
use super::wire::{decode_payload, encode_message, MessageType, WireError};
use crate::executor::{ExecutionError, ExecutionResult};
use crate::p2p::messages::{ResultDeliveryMode, Signature64, WorkerCapabilities};
use crate::ticket::{ChannelState, PaymentTicket, TicketError, TicketValidator};
use async_trait::async_trait;
use iroh::endpoint::{Connection, SendStream};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Errors that can occur during protocol handling.
#[derive(Debug, Error)]
pub enum ProtocolError {
    /// Wire format error.
    #[error("wire error: {0}")]
    WireError(#[from] WireError),

    /// Environment validation failed.
    #[error("env validation: {0}")]
    EnvValidation(#[from] EnvValidationError),

    /// Ticket validation failed.
    #[error("ticket validation: {0}")]
    TicketValidation(#[from] TicketError),

    /// Connection error.
    #[error("connection error: {0}")]
    ConnectionError(String),

    /// Stream closed unexpectedly.
    #[error("stream closed")]
    StreamClosed,

    /// Unexpected message type received.
    #[error("unexpected message type: expected {expected:?}, got {actual:?}")]
    UnexpectedMessageType {
        expected: MessageType,
        actual: MessageType,
    },

    /// Job was rejected.
    #[error("job rejected: {0}")]
    JobRejected(RejectReason),

    /// Internal error.
    #[error("internal error: {0}")]
    InternalError(String),
}

/// Context for job validation and execution.
#[async_trait]
pub trait JobContext: Send + Sync {
    /// Get the worker's current capabilities.
    fn capabilities(&self) -> &WorkerCapabilities;

    /// Get the current available job slots.
    fn available_slots(&self) -> u8;

    /// Get channel state for ticket validation.
    async fn get_channel_state(&self, channel_id: &[u8; 32]) -> Option<ChannelState>;

    /// Get the payer's public key for a channel.
    async fn get_payer_pubkey(&self, channel_id: &[u8; 32]) -> Option<[u8; 32]>;

    /// Called when a job is accepted - should reserve a slot.
    /// Used for async delivery mode where execution is spawned in background.
    /// The `client_node_id` is the client's Ed25519 public key for blob downloads.
    async fn on_job_accepted(&self, job_id: Uuid, request: &JobRequest, client_node_id: [u8; 32]);

    /// Execute a job synchronously, returning the result directly.
    /// Used for sync delivery mode where result is sent on the same stream.
    /// Returns the execution result and job status for wire protocol.
    /// The `client_node_id` is the client's Ed25519 public key for blob downloads.
    async fn execute_job_sync(
        &self,
        job_id: Uuid,
        request: &JobRequest,
        client_node_id: [u8; 32],
    ) -> Result<(ExecutionResult, JobStatus), ExecutionError>;

    /// Called when a job is rejected.
    async fn on_job_rejected(&self, job_id: Uuid, reason: RejectReason);
}

/// Handler for the job submission protocol.
///
/// This handler processes incoming job requests, validates them,
/// and sends appropriate responses.
pub struct JobProtocolHandler<V: TicketValidator, C: JobContext> {
    /// Ticket validator instance.
    validator: Arc<V>,

    /// Job context for capabilities and state.
    context: Arc<C>,
}

impl<V: TicketValidator, C: JobContext> JobProtocolHandler<V, C> {
    /// Create a new protocol handler.
    pub fn new(validator: Arc<V>, context: Arc<C>) -> Self {
        Self { validator, context }
    }

    /// Get a reference to the validator.
    pub fn validator(&self) -> &Arc<V> {
        &self.validator
    }

    /// Get a reference to the job context.
    pub fn context(&self) -> &Arc<C> {
        &self.context
    }

    /// Handle an incoming connection.
    ///
    /// This accepts a bi-directional stream, reads the job request,
    /// validates it, and sends an acceptance or rejection response.
    pub async fn handle_connection(&self, conn: Connection) -> Result<(), ProtocolError> {
        let remote_id = conn.remote_id();
        debug!("Handling job request from {:?}", remote_id);

        // Accept bi-directional stream
        let (mut send, mut recv) = conn
            .accept_bi()
            .await
            .map_err(|e| ProtocolError::ConnectionError(e.to_string()))?;

        // Read the request message
        let mut buf = vec![0u8; 64 * 1024]; // 64KB initial buffer
        let mut offset = 0;

        loop {
            let n = recv
                .read(&mut buf[offset..])
                .await
                .map_err(|e| ProtocolError::ConnectionError(format!("read error: {}", e)))?;

            match n {
                Some(0) | None => {
                    // Check if we have a complete message before giving up
                    if let Some((msg_type, payload, _consumed)) =
                        super::wire::try_read_message(&buf[..offset])?
                    {
                        if msg_type != MessageType::JobRequest {
                            return Err(ProtocolError::UnexpectedMessageType {
                                expected: MessageType::JobRequest,
                                actual: msg_type,
                            });
                        }

                        let request: JobRequest = decode_payload(&payload)?;
                        let client_node_id: [u8; 32] = *remote_id.as_bytes();
                        return self
                            .process_request(request, &mut send, client_node_id)
                            .await;
                    }
                    return Err(ProtocolError::StreamClosed);
                }
                Some(bytes_read) => offset += bytes_read,
            }

            // Try to parse the message
            if let Some((msg_type, payload, _consumed)) =
                super::wire::try_read_message(&buf[..offset])?
            {
                if msg_type != MessageType::JobRequest {
                    return Err(ProtocolError::UnexpectedMessageType {
                        expected: MessageType::JobRequest,
                        actual: msg_type,
                    });
                }

                let request: JobRequest = decode_payload(&payload)?;
                let client_node_id: [u8; 32] = *remote_id.as_bytes();
                return self
                    .process_request(request, &mut send, client_node_id)
                    .await;
            }

            // Need more data - grow buffer if needed
            if offset == buf.len() {
                buf.resize(buf.len() * 2, 0);
            }
        }
    }

    /// Process a job request and send response.
    async fn process_request(
        &self,
        request: JobRequest,
        send: &mut SendStream,
        client_node_id: [u8; 32],
    ) -> Result<(), ProtocolError> {
        let job_id = request.job_id;
        info!("Processing job request: {}", job_id);

        // Validate the request
        match self.validate_request(&request).await {
            Ok(()) => {
                // Branch based on delivery mode
                match request.delivery_mode {
                    ResultDeliveryMode::Sync => {
                        // Sync mode: keep stream open, execute job, send result on same stream
                        self.process_sync_job(job_id, &request, send, client_node_id)
                            .await
                    }
                    ResultDeliveryMode::Async => {
                        // Async mode: spawn background execution, close stream after JobAccepted
                        self.process_async_job(job_id, &request, send, client_node_id)
                            .await
                    }
                }
            }
            Err(reason) => {
                // Reject the job
                self.context.on_job_rejected(job_id, reason).await;

                let response = JobResponse {
                    job_id,
                    status: JobStatus::Rejected(reason),
                    result: None,
                    error: Some(reason.to_string()),
                };

                let encoded = encode_message(MessageType::JobRejected, &response)?;
                send.write_all(&encoded)
                    .await
                    .map_err(|e| ProtocolError::ConnectionError(format!("write error: {}", e)))?;
                self.finish_send(send).await?;

                warn!("Job {} rejected: {}", job_id, reason);
                Ok(())
            }
        }
    }

    /// Process a job in sync mode - keep stream open and send result on same stream.
    async fn process_sync_job(
        &self,
        job_id: Uuid,
        request: &JobRequest,
        send: &mut SendStream,
        client_node_id: [u8; 32],
    ) -> Result<(), ProtocolError> {
        // Send JobAccepted first (but don't close stream)
        let accepted_response = JobResponse {
            job_id,
            status: JobStatus::Accepted,
            result: None,
            error: None,
        };

        let encoded = encode_message(MessageType::JobAccepted, &accepted_response)?;
        send.write_all(&encoded)
            .await
            .map_err(|e| ProtocolError::ConnectionError(format!("write error: {}", e)))?;

        info!("Job {} accepted (sync mode), executing...", job_id);

        // Execute job synchronously (blocks until complete)
        match self
            .context
            .execute_job_sync(job_id, request, client_node_id)
            .await
        {
            Ok((exec_result, status)) => {
                // Build JobResult from ExecutionResult
                let job_result = JobResult {
                    result_hash: exec_result.result_hash,
                    result_url: None, // Sync mode doesn't use URLs
                    encrypted_result: Some(exec_result.encrypted_result.clone().into()),
                    encrypted_stdout: Some(exec_result.encrypted_stdout.clone().into()),
                    encrypted_stderr: Some(exec_result.encrypted_stderr.clone().into()),
                    exit_code: exec_result.exit_code,
                    duration_ms: exec_result.duration_ms(),
                    metrics: JobMetrics::default(),
                    worker_signature: Signature64([0u8; 64]), // TODO(#47): Sign with worker key
                };

                // Send JobResult
                self.send_result(send, job_id, job_result, status).await?;

                // Now finish the stream and wait for ack
                self.finish_send(send).await?;

                info!("Job {} completed (sync mode): {:?}", job_id, status);
                Ok(())
            }
            Err(e) => {
                // Send failure result
                let status = JobStatus::Failed;
                let job_result = JobResult {
                    result_hash: iroh_blobs::Hash::from_bytes([0u8; 32]),
                    result_url: None,
                    encrypted_result: None,
                    encrypted_stdout: None,
                    encrypted_stderr: None,
                    exit_code: -1,
                    duration_ms: 0,
                    metrics: JobMetrics::default(),
                    worker_signature: Signature64([0u8; 64]),
                };

                self.send_result(send, job_id, job_result, status).await?;

                self.finish_send(send).await?;

                warn!("Job {} failed (sync mode): {}", job_id, e);
                Err(ProtocolError::InternalError(e.to_string()))
            }
        }
    }

    /// Process a job in async mode - spawn background execution and close stream after JobAccepted.
    async fn process_async_job(
        &self,
        job_id: Uuid,
        request: &JobRequest,
        send: &mut SendStream,
        client_node_id: [u8; 32],
    ) -> Result<(), ProtocolError> {
        // Accept the job (spawns background execution)
        self.context
            .on_job_accepted(job_id, request, client_node_id)
            .await;

        let response = JobResponse {
            job_id,
            status: JobStatus::Accepted,
            result: None,
            error: None,
        };

        let encoded = encode_message(MessageType::JobAccepted, &response)?;
        send.write_all(&encoded)
            .await
            .map_err(|e| ProtocolError::ConnectionError(format!("write error: {}", e)))?;
        self.finish_send(send).await?;

        info!("Job {} accepted (async mode)", job_id);
        Ok(())
    }

    /// Validate a job request.
    ///
    /// Validation order:
    /// 1. Environment variables (fast, local check)
    /// 2. Capacity (fast, local check)
    /// 3. Kernel support (fast, local check)
    /// 4. Resource limits (fast, local check)
    /// 5. Ticket validation (may involve crypto and state lookup)
    async fn validate_request(&self, request: &JobRequest) -> Result<(), RejectReason> {
        // 1. Validate environment variables
        if let Err(e) = validate_env(&request.manifest.env) {
            return Err(match e {
                EnvValidationError::TooLarge { .. } => RejectReason::EnvTooLarge,
                EnvValidationError::InvalidName { .. } | EnvValidationError::EmptyName => {
                    RejectReason::InvalidEnvName
                }
                EnvValidationError::ReservedPrefix { .. } => RejectReason::ReservedEnvPrefix,
            });
        }

        // 2. Check capacity
        if self.context.available_slots() == 0 {
            return Err(RejectReason::CapacityFull);
        }

        // 3. Check kernel support
        let capabilities = self.context.capabilities();
        if !capabilities.kernels.contains(&request.manifest.kernel) {
            return Err(RejectReason::UnsupportedKernel);
        }

        // 4. Check resource limits
        if request.manifest.vcpu > capabilities.max_vcpu {
            return Err(RejectReason::ResourcesExceedLimits);
        }
        if request.manifest.memory_mb > capabilities.max_memory_mb {
            return Err(RejectReason::ResourcesExceedLimits);
        }

        // 5. Validate inline asset sizes
        use crate::p2p::protocol::types::MAX_MESSAGE_SIZE;
        let total_inline_size = request.assets.total_inline_size();
        if total_inline_size > MAX_MESSAGE_SIZE {
            return Err(RejectReason::InlineTooLarge);
        }

        // 6. Validate payment ticket
        self.validate_ticket(&request.ticket).await?;

        Ok(())
    }

    /// Validate a payment ticket.
    async fn validate_ticket(&self, ticket: &PaymentTicket) -> Result<(), RejectReason> {
        // Look up channel state
        let channel_state = self
            .context
            .get_channel_state(&ticket.channel_id)
            .await
            .unwrap_or_default();

        // Look up payer public key
        let payer_pubkey = self
            .context
            .get_payer_pubkey(&ticket.channel_id)
            .await
            .ok_or(RejectReason::TicketInvalid)?;

        // Validate the ticket
        self.validator
            .validate(ticket, &payer_pubkey, &channel_state)
            .await
            .map_err(|e| match e {
                TicketError::InvalidSignature | TicketError::InvalidPublicKey => {
                    RejectReason::TicketInvalid
                }
                TicketError::ReplayedNonce { .. }
                | TicketError::NonCumulativeAmount { .. }
                | TicketError::InsufficientBalance { .. } => RejectReason::ChannelExhausted,
                _ => RejectReason::TicketInvalid,
            })
    }

    /// Send a progress update on an existing stream.
    pub async fn send_progress(
        &self,
        send: &mut SendStream,
        job_id: Uuid,
        kind: ProgressKind,
        percent: Option<u8>,
        message: Option<String>,
    ) -> Result<(), ProtocolError> {
        let progress = JobProgress {
            job_id,
            kind,
            percent,
            message,
        };

        let encoded = encode_message(MessageType::JobProgress, &progress)?;
        send.write_all(&encoded)
            .await
            .map_err(|e| ProtocolError::ConnectionError(format!("write error: {}", e)))?;

        Ok(())
    }

    /// Send a job result on an existing stream.
    pub async fn send_result(
        &self,
        send: &mut SendStream,
        job_id: Uuid,
        result: JobResult,
        status: JobStatus,
    ) -> Result<(), ProtocolError> {
        let response = JobResponse {
            job_id,
            status,
            result: Some(result),
            error: None,
        };

        let encoded = encode_message(MessageType::JobResult, &response)?;
        send.write_all(&encoded)
            .await
            .map_err(|e| ProtocolError::ConnectionError(format!("write error: {}", e)))?;

        Ok(())
    }

    async fn finish_send(&self, send: &mut SendStream) -> Result<(), ProtocolError> {
        send.finish()
            .map_err(|e| ProtocolError::ConnectionError(format!("finish error: {}", e)))?;
        send.stopped()
            .await
            .map_err(|e| ProtocolError::ConnectionError(format!("stopped error: {}", e)))?;
        Ok(())
    }
}

/// Mock job context for testing.
#[cfg(test)]
pub mod mock {
    use super::*;
    use std::collections::HashMap;
    use tokio::sync::RwLock;

    /// Mock implementation of JobContext for testing.
    pub struct MockJobContext {
        capabilities: WorkerCapabilities,
        available_slots: RwLock<u8>,
        channel_states: RwLock<HashMap<[u8; 32], ChannelState>>,
        payer_pubkeys: RwLock<HashMap<[u8; 32], [u8; 32]>>,
        accepted_jobs: RwLock<Vec<Uuid>>,
        rejected_jobs: RwLock<Vec<(Uuid, RejectReason)>>,
    }

    impl MockJobContext {
        pub fn new(capabilities: WorkerCapabilities, slots: u8) -> Self {
            Self {
                capabilities,
                available_slots: RwLock::new(slots),
                channel_states: RwLock::new(HashMap::new()),
                payer_pubkeys: RwLock::new(HashMap::new()),
                accepted_jobs: RwLock::new(Vec::new()),
                rejected_jobs: RwLock::new(Vec::new()),
            }
        }

        pub async fn set_channel_state(&self, channel_id: [u8; 32], state: ChannelState) {
            self.channel_states.write().await.insert(channel_id, state);
        }

        pub async fn set_payer_pubkey(&self, channel_id: [u8; 32], pubkey: [u8; 32]) {
            self.payer_pubkeys.write().await.insert(channel_id, pubkey);
        }

        pub async fn set_available_slots(&self, slots: u8) {
            *self.available_slots.write().await = slots;
        }

        pub async fn accepted_jobs(&self) -> Vec<Uuid> {
            self.accepted_jobs.read().await.clone()
        }

        pub async fn rejected_jobs(&self) -> Vec<(Uuid, RejectReason)> {
            self.rejected_jobs.read().await.clone()
        }
    }

    #[async_trait]
    impl JobContext for MockJobContext {
        fn capabilities(&self) -> &WorkerCapabilities {
            &self.capabilities
        }

        fn available_slots(&self) -> u8 {
            // Use try_read to avoid blocking in sync context
            self.available_slots
                .try_read()
                .map(|guard| *guard)
                .unwrap_or(0)
        }

        async fn get_channel_state(&self, channel_id: &[u8; 32]) -> Option<ChannelState> {
            self.channel_states.read().await.get(channel_id).cloned()
        }

        async fn get_payer_pubkey(&self, channel_id: &[u8; 32]) -> Option<[u8; 32]> {
            self.payer_pubkeys.read().await.get(channel_id).copied()
        }

        async fn on_job_accepted(
            &self,
            job_id: Uuid,
            _request: &JobRequest,
            _client_node_id: [u8; 32],
        ) {
            self.accepted_jobs.write().await.push(job_id);
            let mut slots = self.available_slots.write().await;
            if *slots > 0 {
                *slots -= 1;
            }
        }

        async fn execute_job_sync(
            &self,
            job_id: Uuid,
            _request: &JobRequest,
            _client_node_id: [u8; 32],
        ) -> Result<(ExecutionResult, JobStatus), ExecutionError> {
            // Reserve slot
            {
                let mut slots = self.available_slots.write().await;
                if *slots > 0 {
                    *slots -= 1;
                }
            }
            self.accepted_jobs.write().await.push(job_id);

            // Return mock successful result
            let result = ExecutionResult::new(
                0, // exit_code
                std::time::Duration::from_millis(100),
                vec![], // encrypted_result
                vec![], // encrypted_stdout
                vec![], // encrypted_stderr
                iroh_blobs::Hash::from_bytes([0u8; 32]),
            );

            // Release slot after execution
            {
                let mut slots = self.available_slots.write().await;
                *slots += 1;
            }

            Ok((result, JobStatus::Succeeded))
        }

        async fn on_job_rejected(&self, job_id: Uuid, reason: RejectReason) {
            self.rejected_jobs.write().await.push((job_id, reason));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mock::MockJobContext;
    use super::*;
    use crate::p2p::messages::{JobManifest, ResultDeliveryMode};
    use crate::p2p::protocol::types::JobAssets;
    use crate::ticket::{MockTicketValidator, MockValidatorBehavior};
    use iroh_blobs::Hash;
    use std::collections::HashMap;

    fn create_test_request() -> JobRequest {
        JobRequest {
            job_id: Uuid::new_v4(),
            manifest: JobManifest {
                vcpu: 1,
                memory_mb: 256,
                timeout_ms: 10000,
                kernel: "python:3.12".to_string(),
                egress_allowlist: vec![],
                env: HashMap::new(),
                estimated_egress_mb: None,
                estimated_ingress_mb: None,
            },
            ticket: PaymentTicket::new([1u8; 32], 1_000_000, 1, 1700000000, [0u8; 64]),
            assets: JobAssets::blobs(Hash::from_bytes([0u8; 32]), None),
            ephemeral_pubkey: [0u8; 32],
            channel_pda: [0u8; 32],
            delivery_mode: ResultDeliveryMode::Sync,
        }
    }

    fn create_test_context() -> MockJobContext {
        let capabilities = WorkerCapabilities {
            max_vcpu: 4,
            max_memory_mb: 4096,
            kernels: vec!["python:3.12".to_string(), "node:21".to_string()],
            disk: None,
            gpus: vec![],
        };
        MockJobContext::new(capabilities, 2)
    }

    #[tokio::test]
    async fn test_validate_request_success() {
        let validator = Arc::new(MockTicketValidator::new(MockValidatorBehavior::AlwaysValid));
        let context = Arc::new(create_test_context());

        // Set up channel state and payer pubkey
        context
            .set_channel_state(
                [1u8; 32],
                ChannelState {
                    last_nonce: 0,
                    last_amount: 0,
                    channel_balance: 10_000_000,
                },
            )
            .await;
        context.set_payer_pubkey([1u8; 32], [42u8; 32]).await;

        let handler = JobProtocolHandler::new(validator, context);
        let request = create_test_request();

        let result = handler.validate_request(&request).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_request_invalid_env_name() {
        let validator = Arc::new(MockTicketValidator::new(MockValidatorBehavior::AlwaysValid));
        let context = Arc::new(create_test_context());

        let handler = JobProtocolHandler::new(validator, context);
        let mut request = create_test_request();
        request
            .manifest
            .env
            .insert("123invalid".to_string(), "value".to_string());

        let result = handler.validate_request(&request).await;
        assert_eq!(result, Err(RejectReason::InvalidEnvName));
    }

    #[tokio::test]
    async fn test_validate_request_reserved_env_prefix() {
        let validator = Arc::new(MockTicketValidator::new(MockValidatorBehavior::AlwaysValid));
        let context = Arc::new(create_test_context());

        let handler = JobProtocolHandler::new(validator, context);
        let mut request = create_test_request();
        request
            .manifest
            .env
            .insert("GRAPHENE_SECRET".to_string(), "value".to_string());

        let result = handler.validate_request(&request).await;
        assert_eq!(result, Err(RejectReason::ReservedEnvPrefix));
    }

    #[tokio::test]
    async fn test_validate_request_capacity_full() {
        let validator = Arc::new(MockTicketValidator::new(MockValidatorBehavior::AlwaysValid));
        let context = Arc::new(create_test_context());
        context.set_available_slots(0).await;

        let handler = JobProtocolHandler::new(validator, context);
        let request = create_test_request();

        let result = handler.validate_request(&request).await;
        assert_eq!(result, Err(RejectReason::CapacityFull));
    }

    #[tokio::test]
    async fn test_validate_request_unsupported_kernel() {
        let validator = Arc::new(MockTicketValidator::new(MockValidatorBehavior::AlwaysValid));
        let context = Arc::new(create_test_context());

        let handler = JobProtocolHandler::new(validator, context);
        let mut request = create_test_request();
        request.manifest.kernel = "rust:1.75".to_string();

        let result = handler.validate_request(&request).await;
        assert_eq!(result, Err(RejectReason::UnsupportedKernel));
    }

    #[tokio::test]
    async fn test_validate_request_resources_exceed_limits() {
        let validator = Arc::new(MockTicketValidator::new(MockValidatorBehavior::AlwaysValid));
        let context = Arc::new(create_test_context());

        let handler = JobProtocolHandler::new(validator, context);
        let mut request = create_test_request();
        request.manifest.vcpu = 8; // Exceeds max of 4

        let result = handler.validate_request(&request).await;
        assert_eq!(result, Err(RejectReason::ResourcesExceedLimits));
    }

    #[tokio::test]
    async fn test_validate_request_memory_exceeds_limits() {
        let validator = Arc::new(MockTicketValidator::new(MockValidatorBehavior::AlwaysValid));
        let context = Arc::new(create_test_context());

        let handler = JobProtocolHandler::new(validator, context);
        let mut request = create_test_request();
        request.manifest.memory_mb = 8192; // Exceeds max of 4096

        let result = handler.validate_request(&request).await;
        assert_eq!(result, Err(RejectReason::ResourcesExceedLimits));
    }

    #[tokio::test]
    async fn test_validate_request_invalid_ticket() {
        let validator = Arc::new(MockTicketValidator::new(
            MockValidatorBehavior::AlwaysInvalidSignature,
        ));
        let context = Arc::new(create_test_context());
        context.set_payer_pubkey([1u8; 32], [42u8; 32]).await;

        let handler = JobProtocolHandler::new(validator, context);
        let request = create_test_request();

        let result = handler.validate_request(&request).await;
        assert_eq!(result, Err(RejectReason::TicketInvalid));
    }

    #[tokio::test]
    async fn test_validate_request_missing_payer_pubkey() {
        let validator = Arc::new(MockTicketValidator::new(MockValidatorBehavior::AlwaysValid));
        let context = Arc::new(create_test_context());
        // Don't set payer pubkey

        let handler = JobProtocolHandler::new(validator, context);
        let request = create_test_request();

        let result = handler.validate_request(&request).await;
        assert_eq!(result, Err(RejectReason::TicketInvalid));
    }
}
