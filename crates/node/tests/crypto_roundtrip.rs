use monad_node::crypto::{CryptoProvider, DefaultCryptoProvider, EncryptionDirection};
use rand::RngCore;

fn random_keypair() -> ([u8; 32], [u8; 32]) {
    let mut secret = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut secret);
    let public = ed25519_dalek::SigningKey::from_bytes(&secret)
        .verifying_key()
        .to_bytes();
    (secret, public)
}

#[test]
fn channel_keys_symmetric_and_job_encryption_roundtrip() {
    let crypto = DefaultCryptoProvider;

    let (user_secret, user_pub) = random_keypair();
    let (worker_secret, worker_pub) = random_keypair();
    let channel_pda = [7u8; 32];

    // Both sides derive the same channel keys
    let user_keys = crypto
        .derive_channel_keys(&user_secret, &worker_pub, &channel_pda)
        .expect("user derive");
    let worker_keys = crypto
        .derive_channel_keys(&worker_secret, &user_pub, &channel_pda)
        .expect("worker derive");

    assert_eq!(
        user_keys.master_key(),
        worker_keys.master_key(),
        "Channel master keys should match"
    );

    // Encrypt input from user -> worker, then decrypt on worker side
    let job_id = "job-123";
    let plaintext = b"hello graphene";
    let encrypted = crypto
        .encrypt_job_blob(plaintext, &user_keys, job_id, EncryptionDirection::Input)
        .expect("encrypt input");
    let decrypted = crypto
        .decrypt_job_blob(&encrypted, &worker_keys, job_id, EncryptionDirection::Input)
        .expect("decrypt input");
    assert_eq!(decrypted, plaintext);

    // Encrypt output from worker -> user, then decrypt on user side
    let output = b"result bytes";
    let enc_out = crypto
        .encrypt_job_blob(output, &worker_keys, job_id, EncryptionDirection::Output)
        .expect("encrypt output");
    let dec_out = crypto
        .decrypt_job_blob(&enc_out, &user_keys, job_id, EncryptionDirection::Output)
        .expect("decrypt output");
    assert_eq!(dec_out, output);

    // Ensure different directions are not interchangeable
    let wrong_dir = crypto.decrypt_job_blob(
        &encrypted,
        &worker_keys,
        job_id,
        EncryptionDirection::Output,
    );
    assert!(wrong_dir.is_err(), "direction mismatch should fail");
}
