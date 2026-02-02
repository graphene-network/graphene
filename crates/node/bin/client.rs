use anyhow::Result;
use iroh::SecretKey;
use iroh::endpoint::Endpoint;
use monad_node::protocol::{Message, TALOS_ALPN};
use std::env;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use rand::rngs::OsRng;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    // MODE 1: CLIENT (Sender)
    // Usage: cargo run -- client <TARGET_PEER_ID>
    if args.len() > 1 && args[1] == "client" {
        let target_peer_str = &args[2];
        let target_peer: iroh::PublicKey = target_peer_str.parse()?;

        run_client(target_peer).await?;
        return Ok(());
    }

    println!("Usage: monad-client client <TARGET_PEER_ID>");
    Ok(())
}

async fn run_client(target: iroh::PublicKey) -> Result<()> {
    println!("🚀 Client Mode. Dialing {}...", target);

    // 1. Setup Local Endpoint
    let mut rng = OsRng;
    let secret = SecretKey::generate(&mut rng);
    let endpoint = Endpoint::builder()
        .secret_key(secret)
        .alpns(vec![TALOS_ALPN.to_vec()])
        .bind()
        .await?;

    // 2. Dial the Target
    // Attempt to use iroh::net::NodeAddr if iroh::NodeAddr is missing.
    // If iroh::net doesn't exist, this will fail.
    // We construct a NodeAddr from the PublicKey (NodeId) and no relay/direct addresses.
    // Assuming NodeAddr::new(NodeId) exists.
    let addr = iroh::net::NodeAddr::new(target);

    // Explicit type annotation to help inference
    let conn: iroh::endpoint::Connection = endpoint.connect(addr, TALOS_ALPN).await?;
    println!("✅ Connected!");

    // 3. Open Stream
    let (mut send, mut recv) = conn.open_bi().await?;

    // 4. Send Job
    let job = Message::JobRequest {
        job_id: "job-p2p-1".into(),
        code: "print('hello iroh')".into(),
        requirements: vec!["pandas".into()],
    };
    send.write_all(&serde_json::to_vec(&job)?).await?;
    send.finish()?;

    // 5. Await Result
    let mut buf = Vec::new();
    recv.read_to_end(&mut buf).await?;
    let response: Message = serde_json::from_slice(&buf)?;

    println!("🎉 Result Received: {:?}", response);
    Ok(())
}
