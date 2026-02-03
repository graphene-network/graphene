//! Length-prefixed wire format for job protocol messages.
//!
//! # Wire Format
//!
//! Each message uses the following framing:
//! ```text
//! [4 bytes: length BE] [1 byte: message type] [N bytes: bincode payload]
//! ```
//!
//! The length field includes both the message type byte and payload bytes.
//!
//! # Message Types
//!
//! - 0x01 = JobRequest
//! - 0x02 = JobAccepted
//! - 0x03 = JobProgress
//! - 0x04 = JobResult
//! - 0x05 = JobRejected

use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

/// Maximum message size (16 MB).
pub const MAX_MESSAGE_SIZE: u32 = 16 * 1024 * 1024;

/// Wire protocol message types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageType {
    /// Job submission request.
    JobRequest = 0x01,
    /// Job accepted acknowledgment.
    JobAccepted = 0x02,
    /// Job progress update.
    JobProgress = 0x03,
    /// Job result delivery.
    JobResult = 0x04,
    /// Job rejected response.
    JobRejected = 0x05,
}

impl MessageType {
    /// Convert from byte value.
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x01 => Some(MessageType::JobRequest),
            0x02 => Some(MessageType::JobAccepted),
            0x03 => Some(MessageType::JobProgress),
            0x04 => Some(MessageType::JobResult),
            0x05 => Some(MessageType::JobRejected),
            _ => None,
        }
    }

    /// Convert to byte value.
    pub fn as_byte(&self) -> u8 {
        *self as u8
    }
}

/// Errors that can occur during wire encoding/decoding.
#[derive(Debug, Error)]
pub enum WireError {
    /// Message exceeds maximum allowed size.
    #[error("message too large: {size} bytes (max {MAX_MESSAGE_SIZE})")]
    MessageTooLarge { size: u32 },

    /// Invalid message type byte.
    #[error("unknown message type: 0x{0:02x}")]
    UnknownMessageType(u8),

    /// Message is too short to contain required header.
    #[error("message truncated: expected at least {expected} bytes, got {actual}")]
    MessageTruncated { expected: usize, actual: usize },

    /// Bincode serialization failed.
    #[error("serialization error: {0}")]
    SerializationError(String),

    /// Bincode deserialization failed.
    #[error("deserialization error: {0}")]
    DeserializationError(String),

    /// I/O error during stream read/write.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Encode a message with length-prefixed framing.
///
/// Returns the full wire format: `[4 bytes: length BE] [1 byte: type] [N bytes: bincode payload]`
pub fn encode_message<T: Serialize>(
    msg_type: MessageType,
    payload: &T,
) -> Result<Vec<u8>, WireError> {
    let payload_bytes =
        bincode::serialize(payload).map_err(|e| WireError::SerializationError(e.to_string()))?;

    // Length = 1 byte (type) + payload length
    let content_len = 1 + payload_bytes.len();
    if content_len > MAX_MESSAGE_SIZE as usize {
        return Err(WireError::MessageTooLarge {
            size: content_len as u32,
        });
    }

    let mut buf = Vec::with_capacity(4 + content_len);
    buf.extend_from_slice(&(content_len as u32).to_be_bytes());
    buf.push(msg_type.as_byte());
    buf.extend_from_slice(&payload_bytes);

    Ok(buf)
}

/// Decode the header from a wire message.
///
/// Returns (message_type, payload_bytes) if successful.
pub fn decode_message(data: &[u8]) -> Result<(MessageType, &[u8]), WireError> {
    // Need at least 5 bytes: 4 (length) + 1 (type)
    if data.len() < 5 {
        return Err(WireError::MessageTruncated {
            expected: 5,
            actual: data.len(),
        });
    }

    let length = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;

    if length > MAX_MESSAGE_SIZE as usize {
        return Err(WireError::MessageTooLarge {
            size: length as u32,
        });
    }

    // Check we have enough data
    if data.len() < 4 + length {
        return Err(WireError::MessageTruncated {
            expected: 4 + length,
            actual: data.len(),
        });
    }

    let msg_type_byte = data[4];
    let msg_type = MessageType::from_byte(msg_type_byte)
        .ok_or(WireError::UnknownMessageType(msg_type_byte))?;

    let payload = &data[5..4 + length];

    Ok((msg_type, payload))
}

/// Decode a bincode payload into a typed struct.
pub fn decode_payload<T: DeserializeOwned>(payload: &[u8]) -> Result<T, WireError> {
    bincode::deserialize(payload).map_err(|e| WireError::DeserializationError(e.to_string()))
}

/// Read a length-prefixed message from a buffer.
///
/// Returns `Some((message_type, payload, bytes_consumed))` if a complete message
/// is available, or `None` if more data is needed.
pub fn try_read_message(buf: &[u8]) -> Result<Option<(MessageType, Vec<u8>, usize)>, WireError> {
    if buf.len() < 4 {
        return Ok(None);
    }

    let length = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;

    if length > MAX_MESSAGE_SIZE as usize {
        return Err(WireError::MessageTooLarge {
            size: length as u32,
        });
    }

    let total_len = 4 + length;
    if buf.len() < total_len {
        return Ok(None);
    }

    let (msg_type, payload) = decode_message(&buf[..total_len])?;
    Ok(Some((msg_type, payload.to_vec(), total_len)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestPayload {
        value: u32,
        name: String,
    }

    #[test]
    fn test_message_type_roundtrip() {
        for mt in [
            MessageType::JobRequest,
            MessageType::JobAccepted,
            MessageType::JobProgress,
            MessageType::JobResult,
            MessageType::JobRejected,
        ] {
            let byte = mt.as_byte();
            let recovered = MessageType::from_byte(byte).expect("should decode");
            assert_eq!(mt, recovered);
        }
    }

    #[test]
    fn test_unknown_message_type() {
        assert!(MessageType::from_byte(0x00).is_none());
        assert!(MessageType::from_byte(0x06).is_none());
        assert!(MessageType::from_byte(0xFF).is_none());
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let payload = TestPayload {
            value: 42,
            name: "test".to_string(),
        };

        let encoded = encode_message(MessageType::JobRequest, &payload).expect("encode failed");

        // Check header
        assert!(encoded.len() >= 5);
        let length = u32::from_be_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]);
        assert_eq!(length as usize, encoded.len() - 4);
        assert_eq!(encoded[4], MessageType::JobRequest.as_byte());

        // Decode
        let (msg_type, payload_bytes) = decode_message(&encoded).expect("decode failed");
        assert_eq!(msg_type, MessageType::JobRequest);

        let decoded: TestPayload = decode_payload(payload_bytes).expect("payload decode failed");
        assert_eq!(decoded, payload);
    }

    #[test]
    fn test_message_truncated() {
        let result = decode_message(&[0x00, 0x00]);
        assert!(matches!(result, Err(WireError::MessageTruncated { .. })));
    }

    #[test]
    fn test_message_incomplete_payload() {
        // Length says 100 bytes, but we only provide 10
        let mut data = vec![0x00, 0x00, 0x00, 0x64]; // length = 100
        data.push(0x01); // message type
        data.extend_from_slice(&[0u8; 5]); // only 5 bytes of payload

        let result = decode_message(&data);
        assert!(matches!(result, Err(WireError::MessageTruncated { .. })));
    }

    #[test]
    fn test_try_read_message_incomplete() {
        // Not enough for length header
        assert!(try_read_message(&[0x00, 0x00]).unwrap().is_none());

        // Length header present but payload incomplete
        let mut data = vec![0x00, 0x00, 0x00, 0x10]; // length = 16
        data.push(0x01); // message type
        data.extend_from_slice(&[0u8; 5]); // only 5 bytes, need 15 more

        assert!(try_read_message(&data).unwrap().is_none());
    }

    #[test]
    fn test_try_read_message_complete() {
        let payload = TestPayload {
            value: 123,
            name: "hello".to_string(),
        };

        let encoded = encode_message(MessageType::JobAccepted, &payload).expect("encode failed");

        // Add some extra bytes after (simulating buffered stream)
        let mut buf = encoded.clone();
        buf.extend_from_slice(&[0xFF, 0xFF]);

        let result = try_read_message(&buf).expect("should succeed");
        let (msg_type, payload_bytes, consumed) = result.expect("should have message");

        assert_eq!(msg_type, MessageType::JobAccepted);
        assert_eq!(consumed, encoded.len());

        let decoded: TestPayload = decode_payload(&payload_bytes).expect("payload decode failed");
        assert_eq!(decoded, payload);
    }

    #[test]
    fn test_message_too_large() {
        // Create length that exceeds max
        let data = [0xFF, 0xFF, 0xFF, 0xFF, 0x01];
        let result = decode_message(&data);
        assert!(matches!(result, Err(WireError::MessageTooLarge { .. })));
    }
}
