//! Job submission protocol for QUIC bi-directional streams.
//!
//! This module implements the wire protocol for job submission over Iroh direct
//! connections using bincode serialization and length-prefixed framing.
//!
//! # Protocol Flow
//!
//! ```text
//! Client                              Worker
//!   |                                    |
//!   |--- JobRequest (0x01) ------------>|
//!   |                                    | validate env, ticket, capacity
//!   |<-- JobAccepted (0x02) ------------|  OR
//!   |<-- JobRejected (0x05) ------------|
//!   |                                    |
//!   |<-- JobProgress (0x03) ------------|  (optional status updates)
//!   |                                    |
//!   |<-- JobResult (0x04) --------------|
//!   |                                    |
//! ```
//!
//! # Wire Format
//!
//! Each message uses length-prefixed framing:
//! ```text
//! [4 bytes: length BE] [1 byte: message type] [N bytes: bincode payload]
//! ```

pub mod handler;
pub mod types;
pub mod validation;
pub mod wire;

pub use handler::{JobProtocolHandler, ProtocolError};
pub use types::{
    JobAssets, JobProgress, JobRequest, JobResponse, JobResult, JobStatus, ProgressKind,
    RejectReason,
};
pub use validation::{validate_env, EnvValidationError, ENV_NAME_REGEX, MAX_ENV_SIZE_BYTES};
pub use wire::{decode_message, encode_message, MessageType, WireError};
