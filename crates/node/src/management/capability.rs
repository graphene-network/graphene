//! Capability-based authentication for management API
//!
//! Uses HKDF derivation from node secret key to create scoped capability tokens.
//! Inspired by capability-based security models.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::time::{SystemTime, UNIX_EPOCH};

/// Capability roles with different permission levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Full administrative access
    Admin,
    /// Operational access (no security changes)
    Operator,
    /// Read-only access
    Reader,
}

impl Role {
    /// Check if this role can perform admin actions
    pub fn is_admin(&self) -> bool {
        matches!(self, Role::Admin)
    }

    /// Check if this role can perform operator actions
    pub fn can_operate(&self) -> bool {
        matches!(self, Role::Admin | Role::Operator)
    }

    /// Check if this role can read data
    pub fn can_read(&self) -> bool {
        true // All roles can read
    }

    /// Get required role for a request type
    pub fn required_for(request_type: &str) -> Role {
        match request_type {
            // Admin-only operations
            "generate_capability" | "revoke_capability" | "apply_upgrade" | "reboot" => Role::Admin,
            // Operator operations
            "apply_config" | "register" | "unregister" | "join" | "drain" | "undrain" | "upgrade" => Role::Operator,
            // Read-only operations
            "get_config" | "get_status" | "stream_logs" | "get_metrics" | "list_capabilities" => Role::Reader,
            // Default to admin for unknown
            _ => Role::Admin,
        }
    }
}

impl Display for Role {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::Admin => write!(f, "admin"),
            Role::Operator => write!(f, "operator"),
            Role::Reader => write!(f, "reader"),
        }
    }
}

/// Errors that can occur during capability operations
#[derive(Debug)]
pub enum CapabilityError {
    /// Invalid token format
    InvalidFormat(String),
    /// Token expired
    Expired,
    /// Token revoked
    Revoked,
    /// Insufficient permissions
    InsufficientPermissions { required: Role, actual: Role },
    /// Token validation failed
    ValidationFailed(String),
}

impl std::error::Error for CapabilityError {}

impl Display for CapabilityError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            CapabilityError::InvalidFormat(msg) => write!(f, "Invalid capability format: {}", msg),
            CapabilityError::Expired => write!(f, "Capability token expired"),
            CapabilityError::Revoked => write!(f, "Capability token has been revoked"),
            CapabilityError::InsufficientPermissions { required, actual } => {
                write!(
                    f,
                    "Insufficient permissions: requires {}, has {}",
                    required, actual
                )
            }
            CapabilityError::ValidationFailed(msg) => {
                write!(f, "Capability validation failed: {}", msg)
            }
        }
    }
}

/// Decoded capability token
#[derive(Debug, Clone)]
pub struct Capability {
    /// Token prefix (first 8 hex chars of hash)
    pub prefix: String,
    /// Full token string
    pub token: String,
    /// Granted role
    pub role: Role,
    /// Creation timestamp
    pub created_at: u64,
    /// Expiration timestamp (if set)
    pub expires_at: Option<u64>,
}

impl Capability {
    /// Check if this capability is expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            now > expires_at
        } else {
            false
        }
    }

    /// Check if this capability can perform an action requiring the given role
    pub fn can_perform(&self, required_role: Role) -> bool {
        match required_role {
            Role::Admin => self.role.is_admin(),
            Role::Operator => self.role.can_operate(),
            Role::Reader => self.role.can_read(),
        }
    }
}

/// Capability manager for token generation and validation
pub struct CapabilityManager {
    /// Node secret key (ed25519 private key bytes)
    secret_key: [u8; 32],
    /// Revoked token prefixes
    revoked: HashMap<String, u64>,
}

impl CapabilityManager {
    /// Create a new capability manager from node secret key
    pub fn new(secret_key: [u8; 32]) -> Self {
        Self {
            secret_key,
            revoked: HashMap::new(),
        }
    }

    /// Generate a new capability token
    ///
    /// Token format: `graphene-cap:v1:<role>:<timestamp>:<signature>`
    pub fn generate(&self, role: Role, ttl_days: Option<u32>) -> Capability {
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let expires_at = ttl_days.map(|days| created_at + (days as u64 * 24 * 60 * 60));

        // Create token payload
        let payload = format!("{}:{}:{}", role, created_at, expires_at.unwrap_or(0));

        // Sign the payload using HMAC-SHA256 with derived key
        let signature = self.sign_payload(&payload);

        // Create full token
        let token = format!(
            "graphene-cap:v1:{}:{}:{}:{}",
            role,
            created_at,
            expires_at.unwrap_or(0),
            hex::encode(&signature)
        );

        // Token prefix is first 8 chars of signature hex
        let prefix = hex::encode(&signature[..4]);

        Capability {
            prefix,
            token,
            role,
            created_at,
            expires_at,
        }
    }

    /// Validate a capability token
    pub fn validate(&self, token: &str) -> Result<Capability, CapabilityError> {
        // Parse token format
        let parts: Vec<&str> = token.split(':').collect();
        if parts.len() != 6 {
            return Err(CapabilityError::InvalidFormat(
                "Expected format graphene-cap:v1:role:created:expires:sig".to_string(),
            ));
        }

        if parts[0] != "graphene-cap" || parts[1] != "v1" {
            return Err(CapabilityError::InvalidFormat(
                "Invalid token prefix".to_string(),
            ));
        }

        let role = match parts[2] {
            "admin" => Role::Admin,
            "operator" => Role::Operator,
            "reader" => Role::Reader,
            _ => {
                return Err(CapabilityError::InvalidFormat(format!(
                    "Unknown role: {}",
                    parts[2]
                )))
            }
        };

        let created_at = parts[3]
            .parse::<u64>()
            .map_err(|_| CapabilityError::InvalidFormat("Invalid created timestamp".to_string()))?;

        let expires_at = match parts[4] {
            "0" => None,
            ts => Some(ts.parse::<u64>().map_err(|_| {
                CapabilityError::InvalidFormat("Invalid expires timestamp".to_string())
            })?),
        };

        let signature = hex::decode(parts[5])
            .map_err(|_| CapabilityError::InvalidFormat("Invalid signature hex".to_string()))?;

        // Verify signature
        let payload = format!("{}:{}:{}", role, created_at, expires_at.unwrap_or(0));
        let expected_sig = self.sign_payload(&payload);

        if signature != expected_sig {
            return Err(CapabilityError::ValidationFailed(
                "Signature verification failed".to_string(),
            ));
        }

        let prefix = hex::encode(&signature[..4]);

        // Check if revoked
        if self.revoked.contains_key(&prefix) {
            return Err(CapabilityError::Revoked);
        }

        let cap = Capability {
            prefix,
            token: token.to_string(),
            role,
            created_at,
            expires_at,
        };

        // Check expiration
        if cap.is_expired() {
            return Err(CapabilityError::Expired);
        }

        Ok(cap)
    }

    /// Revoke a capability by its prefix
    pub fn revoke(&mut self, prefix: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.revoked.insert(prefix.to_string(), now);
    }

    /// List all revoked prefixes
    pub fn list_revoked(&self) -> Vec<(&str, u64)> {
        self.revoked.iter().map(|(k, v)| (k.as_str(), *v)).collect()
    }

    /// Sign a payload using HMAC-SHA256 with key derived from secret
    fn sign_payload(&self, payload: &str) -> Vec<u8> {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        type HmacSha256 = Hmac<Sha256>;

        // Derive signing key from secret
        let derived_key = self.derive_key(b"capability-signing");

        let mut mac =
            HmacSha256::new_from_slice(&derived_key).expect("HMAC can take key of any size");
        mac.update(payload.as_bytes());

        mac.finalize().into_bytes().to_vec()
    }

    /// Derive a key using HKDF
    fn derive_key(&self, info: &[u8]) -> [u8; 32] {
        use hkdf::Hkdf;
        use sha2::Sha256;

        let hk = Hkdf::<Sha256>::new(None, &self.secret_key);
        let mut output = [0u8; 32];
        hk.expand(info, &mut output)
            .expect("32 bytes is valid output length");
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_secret_key() -> [u8; 32] {
        [42u8; 32]
    }

    #[test]
    fn test_generate_and_validate() {
        let manager = CapabilityManager::new(test_secret_key());

        let cap = manager.generate(Role::Admin, None);
        assert_eq!(cap.role, Role::Admin);
        assert!(cap.expires_at.is_none());

        // Validate the generated token
        let validated = manager.validate(&cap.token).unwrap();
        assert_eq!(validated.role, Role::Admin);
        assert_eq!(validated.prefix, cap.prefix);
    }

    #[test]
    fn test_role_permissions() {
        let admin = Role::Admin;
        let operator = Role::Operator;
        let reader = Role::Reader;

        assert!(admin.is_admin());
        assert!(admin.can_operate());
        assert!(admin.can_read());

        assert!(!operator.is_admin());
        assert!(operator.can_operate());
        assert!(operator.can_read());

        assert!(!reader.is_admin());
        assert!(!reader.can_operate());
        assert!(reader.can_read());
    }

    #[test]
    fn test_revocation() {
        let mut manager = CapabilityManager::new(test_secret_key());

        let cap = manager.generate(Role::Operator, None);

        // Token is valid before revocation
        assert!(manager.validate(&cap.token).is_ok());

        // Revoke the token
        manager.revoke(&cap.prefix);

        // Token is now invalid
        let result = manager.validate(&cap.token);
        assert!(matches!(result, Err(CapabilityError::Revoked)));
    }

    #[test]
    fn test_invalid_token_format() {
        let manager = CapabilityManager::new(test_secret_key());

        let result = manager.validate("invalid-token");
        assert!(matches!(result, Err(CapabilityError::InvalidFormat(_))));
    }

    #[test]
    fn test_required_role_for_requests() {
        assert_eq!(Role::required_for("get_status"), Role::Reader);
        assert_eq!(Role::required_for("apply_config"), Role::Operator);
        assert_eq!(Role::required_for("generate_capability"), Role::Admin);
    }
}
