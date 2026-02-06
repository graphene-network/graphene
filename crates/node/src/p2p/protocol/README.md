# Job Submission Protocol

Wire protocol for job submission over Iroh QUIC direct connections.

## Protocol Flow

```
Client                              Worker
  |                                    |
  |--- JobRequest (0x01) ------------>|
  |                                    | validate env, ticket, capacity
  |<-- JobAccepted (0x02) ------------|  OR
  |<-- JobRejected (0x05) ------------|
  |                                    |
  |<-- JobProgress (0x03) ------------|  (optional status updates)
  |                                    |
  |<-- JobResult (0x04) --------------|
  |                                    |
```

## Wire Format

Each message uses length-prefixed framing with bincode serialization:

```
┌─────────────────────────────────────────┐
│  Length (4 bytes, big-endian)           │
├─────────────────────────────────────────┤
│  Message Type (1 byte)                  │
├─────────────────────────────────────────┤
│  Payload (bincode-encoded)              │
└─────────────────────────────────────────┘
```

**Message Types:**

| Type | Value | Description |
|------|-------|-------------|
| JobRequest | 0x01 | Client submits job |
| JobAccepted | 0x02 | Worker accepted job |
| JobProgress | 0x03 | Status update |
| JobResult | 0x04 | Final result |
| JobRejected | 0x05 | Worker rejected job |

**Limits:**
- Max message size: 16 MB

## Message Types

### JobRequest

```rust
pub struct JobRequest {
    pub job_id: Uuid,
    pub manifest: JobManifest,
    pub ticket: PaymentTicket,
    pub assets: JobAssets,
    pub ephemeral_pubkey: [u8; 32],
    pub channel_pda: [u8; 32],
    pub delivery_mode: ResultDeliveryMode,
}
```

### JobResponse

```rust
pub struct JobResponse {
    pub job_id: Uuid,
    pub status: JobStatus,
    pub result: Option<JobResult>,
    pub error: Option<String>,
}

pub enum JobStatus {
    Accepted,
    Running,
    Succeeded,
    Failed,
    Timeout,
    Rejected(RejectReason),
}
```

### Rejection Reasons

| Reason | Description |
|--------|-------------|
| `TicketInvalid` | Payment ticket signature or format invalid |
| `ChannelExhausted` | Payment channel balance exhausted or nonce replayed |
| `CapacityFull` | Worker has no available slots |
| `UnsupportedKernel` | Requested kernel not supported |
| `ResourcesExceedLimits` | Requested vCPU/memory exceeds worker limits |
| `EnvTooLarge` | Environment variables exceed 128KB |
| `InvalidEnvName` | Environment variable name invalid |
| `ReservedEnvPrefix` | Cannot use GRAPHENE_* prefix |
| `AssetUnavailable` | Code or input blob not found |
| `InternalError` | Worker internal error |

## Environment Variables

Jobs can include environment variables in the manifest:

```rust
pub struct JobManifest {
    // ...
    pub env: HashMap<String, String>,
}
```

**Validation Rules:**

1. **Name format:** Must match `^[A-Za-z_][A-Za-z0-9_]*$`
2. **Size limit:** Total size of all keys + values must not exceed 128KB
3. **Reserved prefix:** `GRAPHENE_*` variables cannot be set by users

**System-injected variables:**

| Variable | Value |
|----------|-------|
| `GRAPHENE_JOB_ID` | Job UUID |
| `GRAPHENE_INPUT_PATH` | Path to mounted input (default `/input`) |
| `GRAPHENE_OUTPUT_PATH` | Path for output files (default `/output`) |

## Module Structure

```
protocol/
├── mod.rs          # Module exports
├── types.rs        # Message structs and enums
├── wire.rs         # Length-prefixed bincode framing
├── validation.rs   # Environment variable validation
├── handler.rs      # QUIC stream handler
└── README.md       # This file
```

## Usage

### Worker Side (Handling Requests)

```rust
use graphene_node::p2p::protocol::{JobProtocolHandler, JobContext};

// Implement JobContext for your worker
struct MyWorkerContext { /* ... */ }

impl JobContext for MyWorkerContext {
    fn capabilities(&self) -> &WorkerCapabilities { /* ... */ }
    fn available_slots(&self) -> u8 { /* ... */ }
    // ...
}

// Create handler
let handler = JobProtocolHandler::new(
    Arc::new(ticket_validator),
    Arc::new(worker_context),
);

// In accept loop
if alpn == GRAPHENE_JOB_ALPN {
    handler.handle_connection(conn).await?;
}
```

### Client Side (Submitting Jobs)

```rust
use graphene_node::p2p::protocol::{JobRequest, JobAssets, encode_message, MessageType};

let request = JobRequest {
    job_id: Uuid::new_v4(),
    manifest: JobManifest {
        vcpu: 2,
        memory_mb: 512,
        timeout_ms: 30000,
        kernel: "python:3.12".to_string(),
        egress_allowlist: vec![],
        env: [("API_KEY".into(), "secret".into())].into(),
    },
    ticket: payment_ticket,
    assets: JobAssets {
        code_hash: code_blob_hash,
        code_url: None,
        input_hash: input_blob_hash,
        input_url: None,
    },
    // ...
};

let encoded = encode_message(MessageType::JobRequest, &request)?;
send_stream.write_all(&encoded).await?;
```

## Validation Order

The handler validates requests in this order (fast checks first):

1. Environment variables (local regex + size check)
2. Capacity (local slot check)
3. Kernel support (local capability check)
4. Resource limits (local capability check)
5. Ticket validation (crypto + state lookup)

## Related

- [Issue #23](https://github.com/marcus-sa/graphene/issues/23) - Job submission protocol spec
- `crate::ticket` - Payment ticket validation
- `crate::job::state` - Job lifecycle state machine
