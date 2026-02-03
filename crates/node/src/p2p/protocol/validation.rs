//! Environment variable validation for job submissions.
//!
//! # Validation Rules
//!
//! 1. **Names**: Must match `^[A-Za-z_][A-Za-z0-9_]*$`
//! 2. **Size limit**: Total size of all keys + values must not exceed 128KB
//! 3. **Reserved prefix**: `GRAPHENE_*` variables cannot be set by users

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;
use thiserror::Error;

/// Maximum total size of environment variables in bytes (128 KB).
pub const MAX_ENV_SIZE_BYTES: usize = 128 * 1024;

/// Reserved environment variable prefix.
pub const RESERVED_ENV_PREFIX: &str = "GRAPHENE_";

/// Regex pattern for valid environment variable names.
pub static ENV_NAME_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").unwrap());

/// Errors that can occur during environment variable validation.
#[derive(Debug, Clone, Error)]
pub enum EnvValidationError {
    /// Environment variable name is invalid.
    #[error("invalid environment variable name: '{name}'")]
    InvalidName { name: String },

    /// Environment variable uses reserved GRAPHENE_* prefix.
    #[error("reserved prefix GRAPHENE_* cannot be used: '{name}'")]
    ReservedPrefix { name: String },

    /// Total size of environment variables exceeds limit.
    #[error("environment variables too large: {size} bytes (max {MAX_ENV_SIZE_BYTES})")]
    TooLarge { size: usize },

    /// Environment variable name is empty.
    #[error("environment variable name cannot be empty")]
    EmptyName,
}

/// Validate environment variables for a job submission.
///
/// # Validation Rules
///
/// 1. Names must match `^[A-Za-z_][A-Za-z0-9_]*$`
/// 2. Names cannot start with `GRAPHENE_` (reserved for system use)
/// 3. Total size of all keys + values must not exceed 128KB
///
/// # Arguments
///
/// * `env` - HashMap of environment variable name-value pairs
///
/// # Returns
///
/// `Ok(())` if all variables are valid, or the first validation error found.
pub fn validate_env(env: &HashMap<String, String>) -> Result<(), EnvValidationError> {
    let mut total_size = 0usize;

    for (name, value) in env {
        // Check for empty name
        if name.is_empty() {
            return Err(EnvValidationError::EmptyName);
        }

        // Check name format
        if !ENV_NAME_REGEX.is_match(name) {
            return Err(EnvValidationError::InvalidName { name: name.clone() });
        }

        // Check reserved prefix
        if name.starts_with(RESERVED_ENV_PREFIX) {
            return Err(EnvValidationError::ReservedPrefix { name: name.clone() });
        }

        // Accumulate size
        total_size += name.len() + value.len();
    }

    // Check total size
    if total_size > MAX_ENV_SIZE_BYTES {
        return Err(EnvValidationError::TooLarge { size: total_size });
    }

    Ok(())
}

/// Check if an environment variable name is valid.
///
/// This is a convenience function for validating a single name without
/// checking size limits or reserved prefixes.
pub fn is_valid_env_name(name: &str) -> bool {
    !name.is_empty() && ENV_NAME_REGEX.is_match(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_env_names() {
        // Valid names
        assert!(is_valid_env_name("PATH"));
        assert!(is_valid_env_name("HOME"));
        assert!(is_valid_env_name("_"));
        assert!(is_valid_env_name("_VAR"));
        assert!(is_valid_env_name("VAR_NAME"));
        assert!(is_valid_env_name("var123"));
        assert!(is_valid_env_name("MY_VAR_123"));
        assert!(is_valid_env_name("a"));
        assert!(is_valid_env_name("A"));
    }

    #[test]
    fn test_invalid_env_names() {
        // Invalid names
        assert!(!is_valid_env_name("")); // empty
        assert!(!is_valid_env_name("123")); // starts with number
        assert!(!is_valid_env_name("1VAR")); // starts with number
        assert!(!is_valid_env_name("VAR-NAME")); // hyphen not allowed
        assert!(!is_valid_env_name("VAR.NAME")); // dot not allowed
        assert!(!is_valid_env_name("VAR NAME")); // space not allowed
        assert!(!is_valid_env_name("VAR=VALUE")); // equals not allowed
        assert!(!is_valid_env_name("$VAR")); // dollar not allowed
    }

    #[test]
    fn test_validate_env_valid() {
        let mut env = HashMap::new();
        env.insert("PATH".to_string(), "/usr/bin".to_string());
        env.insert("HOME".to_string(), "/home/user".to_string());
        env.insert("MY_VAR".to_string(), "value".to_string());

        assert!(validate_env(&env).is_ok());
    }

    #[test]
    fn test_validate_env_empty() {
        let env = HashMap::new();
        assert!(validate_env(&env).is_ok());
    }

    #[test]
    fn test_validate_env_invalid_name() {
        let mut env = HashMap::new();
        env.insert("VALID".to_string(), "value".to_string());
        env.insert("123invalid".to_string(), "value".to_string());

        let result = validate_env(&env);
        assert!(matches!(
            result,
            Err(EnvValidationError::InvalidName { .. })
        ));
    }

    #[test]
    fn test_validate_env_empty_name() {
        let mut env = HashMap::new();
        env.insert("".to_string(), "value".to_string());

        let result = validate_env(&env);
        assert!(matches!(result, Err(EnvValidationError::EmptyName)));
    }

    #[test]
    fn test_validate_env_reserved_prefix() {
        let mut env = HashMap::new();
        env.insert("GRAPHENE_SECRET".to_string(), "value".to_string());

        let result = validate_env(&env);
        assert!(matches!(
            result,
            Err(EnvValidationError::ReservedPrefix { .. })
        ));
    }

    #[test]
    fn test_validate_env_reserved_prefix_exact() {
        // Even just "GRAPHENE_" alone is reserved
        let mut env = HashMap::new();
        env.insert("GRAPHENE_".to_string(), "value".to_string());

        let result = validate_env(&env);
        assert!(matches!(
            result,
            Err(EnvValidationError::ReservedPrefix { .. })
        ));
    }

    #[test]
    fn test_validate_env_graphene_prefix_allowed() {
        // "GRAPHENE" without underscore is allowed
        let mut env = HashMap::new();
        env.insert("GRAPHENE".to_string(), "value".to_string());

        assert!(validate_env(&env).is_ok());
    }

    #[test]
    fn test_validate_env_too_large() {
        let mut env = HashMap::new();
        // Create a value that exceeds the limit
        let large_value = "x".repeat(MAX_ENV_SIZE_BYTES + 1);
        env.insert("LARGE".to_string(), large_value);

        let result = validate_env(&env);
        assert!(matches!(result, Err(EnvValidationError::TooLarge { .. })));
    }

    #[test]
    fn test_validate_env_at_limit() {
        let mut env = HashMap::new();
        // Create a value exactly at the limit (accounting for key length)
        let key = "K";
        let value_len = MAX_ENV_SIZE_BYTES - key.len();
        let value = "x".repeat(value_len);
        env.insert(key.to_string(), value);

        assert!(validate_env(&env).is_ok());
    }

    #[test]
    fn test_validate_env_multiple_vars_exceed_limit() {
        let mut env = HashMap::new();
        // Each var is under limit individually, but together they exceed it
        let value = "x".repeat(MAX_ENV_SIZE_BYTES / 2);
        env.insert("VAR1".to_string(), value.clone());
        env.insert("VAR2".to_string(), value);

        let result = validate_env(&env);
        assert!(matches!(result, Err(EnvValidationError::TooLarge { .. })));
    }
}
