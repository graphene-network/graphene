use opencapsule_node::attestation::mock::MockBehavior;
use opencapsule_node::attestation::{enforce_attestation, MockAttestor};

#[tokio::test]
async fn attestation_gating_allows_happy_path() {
    let attestor = MockAttestor::happy_path();
    let result = enforce_attestation(&attestor).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn attestation_gating_rejects_failure() {
    let attestor = MockAttestor::new(MockBehavior::VerityMismatch);
    let result = enforce_attestation(&attestor).await;
    assert!(result.is_err());
}
