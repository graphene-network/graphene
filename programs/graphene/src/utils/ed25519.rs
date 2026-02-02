use anchor_lang::prelude::*;
use anchor_lang::solana_program::sysvar::instructions::{
    load_current_index_checked, load_instruction_at_checked,
};

use crate::error::GrapheneError;

/// Ed25519 program ID bytes (Ed25519SigVerify111111111111111111111111111)
const ED25519_PROGRAM_ID_BYTES: [u8; 32] = [
    0x03, 0x7d, 0x46, 0xd6, 0x7c, 0x93, 0xfb, 0xbe, 0x12, 0xf9, 0x42, 0x8f, 0x83, 0x8d, 0x40, 0xff,
    0x05, 0x70, 0x74, 0x49, 0x27, 0xf4, 0x8a, 0x64, 0xfc, 0xca, 0x70, 0x44, 0x80, 0x00, 0x00, 0x00,
];

/// Expected Ed25519 instruction index in the transaction
pub const ED25519_IX_INDEX: usize = 0;

/// Verify that an Ed25519 signature was validated in a preceding instruction.
///
/// # Arguments
/// * `ix_sysvar` - The instructions sysvar account
/// * `expected_signer` - The expected public key that signed the message
/// * `channel` - The channel pubkey (first 32 bytes of message)
/// * `amount` - The payment amount (next 8 bytes, little-endian)
/// * `nonce` - The nonce (final 8 bytes, little-endian)
///
/// # Message Format
/// The signed message is 48 bytes: `[channel: 32][amount: 8 LE][nonce: 8 LE]`
pub fn verify_ed25519_signature(
    ix_sysvar: &AccountInfo,
    expected_signer: &Pubkey,
    channel: &Pubkey,
    amount: u64,
    nonce: u64,
) -> Result<()> {
    let ed25519_program_id = Pubkey::new_from_array(ED25519_PROGRAM_ID_BYTES);

    // Verify we're not at index 0 (Ed25519 instruction should be before us)
    let current_ix_index = load_current_index_checked(ix_sysvar)?;
    require!(
        current_ix_index > ED25519_IX_INDEX as u16,
        GrapheneError::Ed25519InstructionNotFound
    );

    // Load the Ed25519 instruction
    let ed25519_ix = load_instruction_at_checked(ED25519_IX_INDEX, ix_sysvar)?;

    // Verify it's the Ed25519 program
    require!(
        ed25519_ix.program_id == ed25519_program_id,
        GrapheneError::Ed25519InstructionNotFound
    );

    // Parse Ed25519 instruction data
    // Format: [num_signatures: 1][padding: 1][signature_offset: 2][signature_ix_index: 2]
    //         [pubkey_offset: 2][pubkey_ix_index: 2][message_offset: 2][message_size: 2]
    //         [message_ix_index: 2][signature: 64][pubkey: 32][message: N]
    let ix_data = &ed25519_ix.data;
    require!(
        ix_data.len() >= 16,
        GrapheneError::InvalidEd25519InstructionData
    );

    let num_signatures = ix_data[0];
    require!(
        num_signatures == 1,
        GrapheneError::InvalidEd25519InstructionData
    );

    // Parse offsets (all little-endian u16)
    let signature_offset = u16::from_le_bytes([ix_data[2], ix_data[3]]) as usize;
    let pubkey_offset = u16::from_le_bytes([ix_data[6], ix_data[7]]) as usize;
    let message_offset = u16::from_le_bytes([ix_data[10], ix_data[11]]) as usize;
    let message_size = u16::from_le_bytes([ix_data[12], ix_data[13]]) as usize;

    // Validate data bounds
    require!(
        pubkey_offset + 32 <= ix_data.len(),
        GrapheneError::InvalidEd25519InstructionData
    );
    require!(
        message_offset + message_size <= ix_data.len(),
        GrapheneError::InvalidEd25519InstructionData
    );
    require!(
        signature_offset + 64 <= ix_data.len(),
        GrapheneError::InvalidEd25519InstructionData
    );

    // Extract and verify public key
    let pubkey_bytes = &ix_data[pubkey_offset..pubkey_offset + 32];
    require!(
        pubkey_bytes == expected_signer.as_ref(),
        GrapheneError::SignatureVerificationFailed
    );

    // Verify message size (should be 48 bytes: channel + amount + nonce)
    require!(
        message_size == 48,
        GrapheneError::InvalidEd25519InstructionData
    );

    // Extract and verify message contents
    let message = &ix_data[message_offset..message_offset + message_size];

    // Verify channel (bytes 0-31)
    require!(
        &message[0..32] == channel.as_ref(),
        GrapheneError::SignatureVerificationFailed
    );

    // Verify amount (bytes 32-39, little-endian)
    let msg_amount = u64::from_le_bytes(message[32..40].try_into().unwrap());
    require!(
        msg_amount == amount,
        GrapheneError::SignatureVerificationFailed
    );

    // Verify nonce (bytes 40-47, little-endian)
    let msg_nonce = u64::from_le_bytes(message[40..48].try_into().unwrap());
    require!(
        msg_nonce == nonce,
        GrapheneError::SignatureVerificationFailed
    );

    Ok(())
}
