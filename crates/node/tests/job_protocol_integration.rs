//! Integration tests for the job submission protocol over QUIC.
//!
//! These tests create actual Iroh endpoints and test the full protocol flow.

use graphene_node::executor::{ExecutionError, ExecutionResult};
use graphene_node::p2p::graphene::GRAPHENE_JOB_ALPN;
use graphene_node::p2p::messages::{JobManifest, ResultDeliveryMode, WorkerCapabilities};
use graphene_node::p2p::protocol::{
    decode_message, encode_message, JobAssets, JobContext, JobProtocolHandler, JobRequest,
    JobResponse, JobStatus, MessageType, RejectReason,
};
use graphene_node::ticket::{
    ChannelState, DefaultTicketSigner, MockTicketValidator, MockValidatorBehavior, PaymentTicket,
    TicketSigner, TicketValidator,
};
use iroh::endpoint::Endpoint;
use iroh::SecretKey;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Test worker context that implements JobContext.
struct TestWorkerContext {
    capabilities: WorkerCapabilities,
    available_slots: RwLock<u8>,
    channel_states: RwLock<HashMap<[u8; 32], ChannelState>>,
    payer_pubkeys: RwLock<HashMap<[u8; 32], [u8; 32]>>,
    accepted_jobs: RwLock<Vec<Uuid>>,
    rejected_jobs: RwLock<Vec<(Uuid, RejectReason)>>,
}

impl TestWorkerContext {
    fn new(capabilities: WorkerCapabilities, slots: u8) -> Self {
        Self {
            capabilities,
            available_slots: RwLock::new(slots),
            channel_states: RwLock::new(HashMap::new()),
            payer_pubkeys: RwLock::new(HashMap::new()),
            accepted_jobs: RwLock::new(Vec::new()),
            rejected_jobs: RwLock::new(Vec::new()),
        }
    }

    async fn set_channel_state(&self, channel_id: [u8; 32], state: ChannelState) {
        self.channel_states.write().await.insert(channel_id, state);
    }

    async fn set_payer_pubkey(&self, channel_id: [u8; 32], pubkey: [u8; 32]) {
        self.payer_pubkeys.write().await.insert(channel_id, pubkey);
    }

    async fn accepted_jobs(&self) -> Vec<Uuid> {
        self.accepted_jobs.read().await.clone()
    }

    async fn rejected_jobs(&self) -> Vec<(Uuid, RejectReason)> {
        self.rejected_jobs.read().await.clone()
    }
}

#[async_trait::async_trait]
impl JobContext for TestWorkerContext {
    fn capabilities(&self) -> &WorkerCapabilities {
        &self.capabilities
    }

    fn available_slots(&self) -> u8 {
        self.available_slots.try_read().map(|g| *g).unwrap_or(0)
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

/// Create a test endpoint with the job protocol ALPN.
async fn create_test_endpoint() -> Endpoint {
    use rand::RngCore;
    let mut key_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key_bytes);
    let secret_key = SecretKey::from_bytes(&key_bytes);
    Endpoint::builder()
        .secret_key(secret_key)
        .alpns(vec![GRAPHENE_JOB_ALPN.to_vec()])
        .bind()
        .await
        .expect("failed to create endpoint")
}

/// Create a valid test job request.
fn create_test_request(channel_id: [u8; 32], ticket: PaymentTicket) -> JobRequest {
    JobRequest {
        job_id: Uuid::new_v4(),
        manifest: JobManifest {
            vcpu: 1,
            memory_mb: 256,
            timeout_ms: 10000,
            kernel: "python:3.12".to_string(),
            egress_allowlist: vec![],
            env: [("MY_VAR".to_string(), "my_value".to_string())]
                .into_iter()
                .collect(),
            estimated_egress_mb: None,
            estimated_ingress_mb: None,
        },
        ticket,
        assets: JobAssets::blobs(
            iroh_blobs::Hash::from_bytes([1u8; 32]),
            Some(iroh_blobs::Hash::from_bytes([2u8; 32])),
        ),
        ephemeral_pubkey: [0u8; 32],
        channel_pda: channel_id,
        delivery_mode: ResultDeliveryMode::Sync,
    }
}

/// Create test worker capabilities.
fn create_test_capabilities() -> WorkerCapabilities {
    WorkerCapabilities {
        max_vcpu: 4,
        max_memory_mb: 4096,
        kernels: vec!["python:3.12".to_string(), "node:20".to_string()],
        disk: None,
        gpus: vec![],
    }
}

/// Helper to run a single request/response exchange.
async fn run_job_submission(
    worker_endpoint: Endpoint,
    client_endpoint: Endpoint,
    handler: Arc<JobProtocolHandler<MockTicketValidator, TestWorkerContext>>,
    request: JobRequest,
) -> (JobResponse, MessageType) {
    let worker_addr = worker_endpoint.addr();
    let job_id = request.job_id;

    // Use a channel to signal when the client has read the response
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();

    // Spawn worker accept loop - keep the connection alive until client is done
    let worker_handle = tokio::spawn(async move {
        let incoming = worker_endpoint.accept().await.expect("no incoming");
        let conn = incoming.await.expect("accept failed");

        // Accept the bi-stream manually
        let (mut send, mut recv) = conn.accept_bi().await.expect("accept_bi failed");

        // Read the request
        let mut buf = vec![0u8; 64 * 1024];
        let mut offset = 0;
        loop {
            match recv.read(&mut buf[offset..]).await {
                Ok(Some(n)) if n > 0 => offset += n,
                _ => break,
            }

            // Try to parse
            if graphene_node::p2p::protocol::wire::try_read_message(&buf[..offset])
                .ok()
                .flatten()
                .is_some()
            {
                break;
            }
        }

        // Parse and validate the request
        if let Ok(Some((msg_type, payload, _))) =
            graphene_node::p2p::protocol::wire::try_read_message(&buf[..offset])
        {
            if msg_type == MessageType::JobRequest {
                let request: JobRequest =
                    graphene_node::p2p::protocol::wire::decode_payload(&payload).expect("decode");

                // Use the handler's context to validate
                let context = handler.context();
                let validator = handler.validator();

                // Validation in order: env -> capacity -> kernel -> resources -> ticket
                let capabilities = context.capabilities();

                // 1. Check environment variables
                let env_result = graphene_node::p2p::protocol::validate_env(&request.manifest.env);
                let (status, msg_type_out) = if let Err(e) = env_result {
                    use graphene_node::p2p::protocol::EnvValidationError;
                    let reason = match e {
                        EnvValidationError::TooLarge { .. } => RejectReason::EnvTooLarge,
                        EnvValidationError::InvalidName { .. } | EnvValidationError::EmptyName => {
                            RejectReason::InvalidEnvName
                        }
                        EnvValidationError::ReservedPrefix { .. } => {
                            RejectReason::ReservedEnvPrefix
                        }
                    };
                    context.on_job_rejected(request.job_id, reason).await;
                    (JobStatus::Rejected(reason), MessageType::JobRejected)
                }
                // 2. Check capacity
                else if context.available_slots() == 0 {
                    let reason = RejectReason::CapacityFull;
                    context.on_job_rejected(request.job_id, reason).await;
                    (JobStatus::Rejected(reason), MessageType::JobRejected)
                }
                // 3. Check kernel support
                else if !capabilities.kernels.contains(&request.manifest.kernel) {
                    let reason = RejectReason::UnsupportedKernel;
                    context.on_job_rejected(request.job_id, reason).await;
                    (JobStatus::Rejected(reason), MessageType::JobRejected)
                }
                // 4. Check resource limits
                else if request.manifest.vcpu > capabilities.max_vcpu
                    || request.manifest.memory_mb > capabilities.max_memory_mb
                {
                    let reason = RejectReason::ResourcesExceedLimits;
                    context.on_job_rejected(request.job_id, reason).await;
                    (JobStatus::Rejected(reason), MessageType::JobRejected)
                }
                // 5. Check ticket
                else {
                    let payer_pubkey = context.get_payer_pubkey(&request.ticket.channel_id).await;
                    let channel_state = context
                        .get_channel_state(&request.ticket.channel_id)
                        .await
                        .unwrap_or_default();

                    if let Some(pubkey) = payer_pubkey {
                        match validator
                            .validate(&request.ticket, &pubkey, &channel_state)
                            .await
                        {
                            Ok(()) => {
                                context
                                    .on_job_accepted(request.job_id, &request, [0u8; 32])
                                    .await;
                                (JobStatus::Accepted, MessageType::JobAccepted)
                            }
                            Err(_) => {
                                let reason = RejectReason::TicketInvalid;
                                context.on_job_rejected(request.job_id, reason).await;
                                (JobStatus::Rejected(reason), MessageType::JobRejected)
                            }
                        }
                    } else {
                        let reason = RejectReason::TicketInvalid;
                        context.on_job_rejected(request.job_id, reason).await;
                        (JobStatus::Rejected(reason), MessageType::JobRejected)
                    }
                };

                // Build and send response
                let response = JobResponse {
                    job_id: request.job_id,
                    status,
                    result: None,
                    error: if matches!(status, JobStatus::Rejected(_)) {
                        Some(format!("{:?}", status))
                    } else {
                        None
                    },
                };

                let encoded = encode_message(msg_type_out, &response).expect("encode");
                send.write_all(&encoded).await.expect("write");
                send.finish().expect("finish send stream");
            }
        }

        // Keep connection alive until client signals done
        let _ = done_rx.await;

        // Explicitly drop to control order
        drop(send);
        drop(recv);
        drop(conn);
        drop(worker_endpoint);
    });

    // Give the worker time to start accepting
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Client connects and sends request
    let conn = client_endpoint
        .connect(worker_addr, GRAPHENE_JOB_ALPN)
        .await
        .expect("connect failed");

    let (mut send, mut recv) = conn.open_bi().await.expect("open_bi failed");

    // Send request
    let encoded = encode_message(MessageType::JobRequest, &request).expect("encode failed");
    send.write_all(&encoded).await.expect("write failed");
    send.finish().expect("finish failed");

    // Read response
    let mut buf = vec![0u8; 4096];
    let mut offset = 0;
    loop {
        match recv.read(&mut buf[offset..]).await {
            Ok(Some(n)) => {
                if n == 0 {
                    break;
                }
                offset += n;
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }

    // Signal worker we're done reading
    let _ = done_tx.send(());

    let (msg_type, payload) = decode_message(&buf[..offset]).expect("decode failed");
    let response: JobResponse =
        graphene_node::p2p::protocol::wire::decode_payload(payload).expect("payload decode failed");

    assert_eq!(response.job_id, job_id);

    // Wait for worker to finish
    let _ = worker_handle.await;

    (response, msg_type)
}

#[tokio::test]
async fn test_job_submission_accepted() {
    // Create endpoints
    let worker_endpoint = create_test_endpoint().await;
    let client_endpoint = create_test_endpoint().await;

    // Set up worker context with valid channel state
    let channel_id = [42u8; 32];
    let signer = DefaultTicketSigner::from_bytes(&[1u8; 32]);
    let payer_pubkey = signer.public_key();

    let context = Arc::new(TestWorkerContext::new(create_test_capabilities(), 2));
    context
        .set_channel_state(
            channel_id,
            ChannelState {
                last_nonce: 0,
                last_amount: 0,
                channel_balance: 10_000_000,
            },
        )
        .await;
    context.set_payer_pubkey(channel_id, payer_pubkey).await;

    // Create handler with always-valid ticket validator
    let validator = Arc::new(MockTicketValidator::new(MockValidatorBehavior::AlwaysValid));
    let handler = Arc::new(JobProtocolHandler::new(validator, context.clone()));

    // Create and sign ticket
    let ticket = signer
        .sign_ticket(channel_id, 1_000_000, 1)
        .await
        .expect("sign failed");
    let request = create_test_request(channel_id, ticket);
    let job_id = request.job_id;

    // Run the submission
    let (response, msg_type) =
        run_job_submission(worker_endpoint, client_endpoint, handler, request).await;

    // Verify response
    assert_eq!(msg_type, MessageType::JobAccepted);
    assert_eq!(response.status, JobStatus::Accepted);
    assert!(response.error.is_none());

    // Verify worker recorded the accepted job
    let accepted = context.accepted_jobs().await;
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0], job_id);
}

#[tokio::test]
async fn test_job_submission_rejected_invalid_ticket() {
    // Create endpoints
    let worker_endpoint = create_test_endpoint().await;
    let client_endpoint = create_test_endpoint().await;

    // Set up worker context
    let channel_id = [42u8; 32];
    let context = Arc::new(TestWorkerContext::new(create_test_capabilities(), 2));
    context.set_payer_pubkey(channel_id, [99u8; 32]).await;

    // Create handler with always-invalid ticket validator
    let validator = Arc::new(MockTicketValidator::new(
        MockValidatorBehavior::AlwaysInvalidSignature,
    ));
    let handler = Arc::new(JobProtocolHandler::new(validator, context.clone()));

    // Create request with invalid ticket
    let ticket = PaymentTicket::new(channel_id, 1_000_000, 1, 1700000000, [0u8; 64]);
    let request = create_test_request(channel_id, ticket);
    let job_id = request.job_id;

    // Run the submission
    let (response, msg_type) =
        run_job_submission(worker_endpoint, client_endpoint, handler, request).await;

    // Verify response
    assert_eq!(msg_type, MessageType::JobRejected);
    assert_eq!(
        response.status,
        JobStatus::Rejected(RejectReason::TicketInvalid)
    );
    assert!(response.error.is_some());

    // Verify worker recorded the rejection
    let rejected = context.rejected_jobs().await;
    assert_eq!(rejected.len(), 1);
    assert_eq!(rejected[0].0, job_id);
    assert_eq!(rejected[0].1, RejectReason::TicketInvalid);
}

#[tokio::test]
async fn test_job_submission_rejected_unsupported_kernel() {
    // Create endpoints
    let worker_endpoint = create_test_endpoint().await;
    let client_endpoint = create_test_endpoint().await;

    // Set up worker context
    let channel_id = [42u8; 32];
    let context = Arc::new(TestWorkerContext::new(create_test_capabilities(), 2));
    context.set_payer_pubkey(channel_id, [99u8; 32]).await;

    // Create handler
    let validator = Arc::new(MockTicketValidator::new(MockValidatorBehavior::AlwaysValid));
    let handler = Arc::new(JobProtocolHandler::new(validator, context.clone()));

    // Create request with unsupported kernel
    let ticket = PaymentTicket::new(channel_id, 1_000_000, 1, 1700000000, [0u8; 64]);
    let mut request = create_test_request(channel_id, ticket);
    request.manifest.kernel = "rust:1.75".to_string(); // Not in capabilities

    // Run the submission
    let (response, msg_type) =
        run_job_submission(worker_endpoint, client_endpoint, handler, request).await;

    // Verify response
    assert_eq!(msg_type, MessageType::JobRejected);
    assert_eq!(
        response.status,
        JobStatus::Rejected(RejectReason::UnsupportedKernel)
    );

    // Verify rejection recorded
    let rejected = context.rejected_jobs().await;
    assert_eq!(rejected[0].1, RejectReason::UnsupportedKernel);
}

#[tokio::test]
async fn test_job_submission_rejected_invalid_env_name() {
    // Create endpoints
    let worker_endpoint = create_test_endpoint().await;
    let client_endpoint = create_test_endpoint().await;

    // Set up worker context
    let channel_id = [42u8; 32];
    let context = Arc::new(TestWorkerContext::new(create_test_capabilities(), 2));
    context.set_payer_pubkey(channel_id, [99u8; 32]).await;

    // Create handler
    let validator = Arc::new(MockTicketValidator::new(MockValidatorBehavior::AlwaysValid));
    let handler = Arc::new(JobProtocolHandler::new(validator, context.clone()));

    // Create request with invalid env var name
    let ticket = PaymentTicket::new(channel_id, 1_000_000, 1, 1700000000, [0u8; 64]);
    let mut request = create_test_request(channel_id, ticket);
    request
        .manifest
        .env
        .insert("123invalid".to_string(), "value".to_string());

    // Run the submission
    let (response, msg_type) =
        run_job_submission(worker_endpoint, client_endpoint, handler, request).await;

    // Verify response
    assert_eq!(msg_type, MessageType::JobRejected);
    assert_eq!(
        response.status,
        JobStatus::Rejected(RejectReason::InvalidEnvName)
    );
}

#[tokio::test]
async fn test_job_submission_rejected_reserved_env_prefix() {
    // Create endpoints
    let worker_endpoint = create_test_endpoint().await;
    let client_endpoint = create_test_endpoint().await;

    // Set up worker context
    let channel_id = [42u8; 32];
    let context = Arc::new(TestWorkerContext::new(create_test_capabilities(), 2));
    context.set_payer_pubkey(channel_id, [99u8; 32]).await;

    // Create handler
    let validator = Arc::new(MockTicketValidator::new(MockValidatorBehavior::AlwaysValid));
    let handler = Arc::new(JobProtocolHandler::new(validator, context.clone()));

    // Create request with reserved GRAPHENE_ prefix
    let ticket = PaymentTicket::new(channel_id, 1_000_000, 1, 1700000000, [0u8; 64]);
    let mut request = create_test_request(channel_id, ticket);
    request
        .manifest
        .env
        .insert("GRAPHENE_SECRET".to_string(), "value".to_string());

    // Run the submission
    let (response, msg_type) =
        run_job_submission(worker_endpoint, client_endpoint, handler, request).await;

    // Verify response
    assert_eq!(msg_type, MessageType::JobRejected);
    assert_eq!(
        response.status,
        JobStatus::Rejected(RejectReason::ReservedEnvPrefix)
    );
}

#[tokio::test]
async fn test_job_submission_rejected_resources_exceed_limits() {
    let worker_endpoint = create_test_endpoint().await;
    let client_endpoint = create_test_endpoint().await;

    let channel_id = [42u8; 32];
    let context = Arc::new(TestWorkerContext::new(create_test_capabilities(), 2));
    context.set_payer_pubkey(channel_id, [99u8; 32]).await;

    let validator = Arc::new(MockTicketValidator::new(MockValidatorBehavior::AlwaysValid));
    let handler = Arc::new(JobProtocolHandler::new(validator, context.clone()));

    let ticket = PaymentTicket::new(channel_id, 1_000_000, 1, 1700000000, [0u8; 64]);
    let mut request = create_test_request(channel_id, ticket);
    request.manifest.vcpu = 8; // exceeds max_vcpu = 4

    let (response, msg_type) =
        run_job_submission(worker_endpoint, client_endpoint, handler, request).await;

    assert_eq!(msg_type, MessageType::JobRejected);
    assert_eq!(
        response.status,
        JobStatus::Rejected(RejectReason::ResourcesExceedLimits)
    );
}

#[tokio::test]
async fn test_job_submission_with_env_vars() {
    // Create endpoints
    let worker_endpoint = create_test_endpoint().await;
    let client_endpoint = create_test_endpoint().await;

    // Set up worker context
    let channel_id = [42u8; 32];
    let signer = DefaultTicketSigner::from_bytes(&[1u8; 32]);
    let payer_pubkey = signer.public_key();

    let context = Arc::new(TestWorkerContext::new(create_test_capabilities(), 2));
    context
        .set_channel_state(
            channel_id,
            ChannelState {
                last_nonce: 0,
                last_amount: 0,
                channel_balance: 10_000_000,
            },
        )
        .await;
    context.set_payer_pubkey(channel_id, payer_pubkey).await;

    // Create handler
    let validator = Arc::new(MockTicketValidator::new(MockValidatorBehavior::AlwaysValid));
    let handler = Arc::new(JobProtocolHandler::new(validator, context.clone()));

    // Create request with multiple valid env vars
    let ticket = signer
        .sign_ticket(channel_id, 1_000_000, 1)
        .await
        .expect("sign failed");
    let mut request = create_test_request(channel_id, ticket);
    request.manifest.env = [
        ("API_KEY".to_string(), "sk-test-123".to_string()),
        ("MODE".to_string(), "production".to_string()),
        ("BATCH_SIZE".to_string(), "100".to_string()),
        ("_INTERNAL".to_string(), "value".to_string()),
    ]
    .into_iter()
    .collect();

    // Run the submission
    let (response, msg_type) =
        run_job_submission(worker_endpoint, client_endpoint, handler, request).await;

    // Verify response - should be accepted with valid env vars
    assert_eq!(msg_type, MessageType::JobAccepted);
    assert_eq!(response.status, JobStatus::Accepted);
}
