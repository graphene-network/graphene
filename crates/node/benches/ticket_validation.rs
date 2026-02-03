//! Benchmarks for payment ticket validation.
//!
//! Target: < 1ms per verification (issue #27 requirement).

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use monad_node::ticket::{
    ChannelState, DefaultTicketSigner, DefaultTicketValidator, TicketSigner, TicketValidator,
};
use tokio::runtime::Runtime;

fn create_test_ticket(rt: &Runtime) -> (monad_node::ticket::PaymentTicket, [u8; 32]) {
    let secret = [42u8; 32];
    let signer = DefaultTicketSigner::from_bytes(&secret);

    let ticket = rt
        .block_on(signer.sign_ticket([1u8; 32], 1_000_000, 5))
        .expect("signing failed");

    (ticket, signer.public_key())
}

fn bench_ticket_signing(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let secret = [42u8; 32];
    let signer = DefaultTicketSigner::from_bytes(&secret);

    c.bench_function("ticket_signing", |b| {
        b.iter(|| {
            rt.block_on(signer.sign_ticket(
                black_box([1u8; 32]),
                black_box(1_000_000),
                black_box(5),
            ))
            .expect("signing failed")
        })
    });
}

fn bench_ticket_validation(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let (ticket, pubkey) = create_test_ticket(&rt);
    let validator = DefaultTicketValidator::new();

    let channel_state = ChannelState {
        last_nonce: 4,
        last_amount: 500_000,
        channel_balance: 10_000_000,
    };

    c.bench_function("ticket_validation", |b| {
        b.iter(|| {
            rt.block_on(validator.validate(
                black_box(&ticket),
                black_box(&pubkey),
                black_box(&channel_state),
            ))
            .expect("validation failed")
        })
    });
}

fn bench_signature_verification_only(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let (ticket, pubkey) = create_test_ticket(&rt);

    c.bench_function("ed25519_verify_only", |b| {
        b.iter(|| {
            let verifying_key =
                ed25519_dalek::VerifyingKey::from_bytes(black_box(&pubkey)).unwrap();
            let signature = ed25519_dalek::Signature::from_bytes(black_box(ticket.signature()));
            let message = ticket.signed_message();
            ed25519_dalek::Verifier::verify(&verifying_key, black_box(&message), &signature)
                .expect("verification failed")
        })
    });
}

fn bench_payload_serialization(c: &mut Criterion) {
    use monad_node::ticket::TicketPayload;

    let payload = TicketPayload {
        channel_id: [42u8; 32],
        amount_micros: 1_000_000,
        nonce: 5,
    };

    c.bench_function("payload_to_bytes", |b| {
        b.iter(|| black_box(&payload).to_bytes())
    });

    let bytes = payload.to_bytes();
    c.bench_function("payload_from_bytes", |b| {
        b.iter(|| TicketPayload::from_bytes(black_box(&bytes)))
    });
}

criterion_group!(
    benches,
    bench_ticket_signing,
    bench_ticket_validation,
    bench_signature_verification_only,
    bench_payload_serialization,
);

criterion_main!(benches);
