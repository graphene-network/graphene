use anyhow::Result;
use iroh::{EndpointAddr, SecretKey};
use iroh::endpoint::Endpoint;
use graphene_node::protocol::{Message, TALOS_ALPN};
use rand::RngCore;
use std::env;

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

    println!("Usage: graphene-client client <TARGET_PEER_ID>");
    Ok(())
}

async fn run_client(target: iroh::PublicKey) -> Result<()> {
    println!("🚀 Client Mode. Dialing {}...", target);

    // 1. Setup Local Endpoint
    let mut key_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key_bytes);
    let secret = SecretKey::from_bytes(&key_bytes);
    let endpoint = Endpoint::builder()
        .secret_key(secret)
        .alpns(vec![TALOS_ALPN.to_vec()])
        .bind()
        .await?;

    // 2. Dial the Target
    // Construct an EndpointAddr from the PublicKey
    let addr = EndpointAddr::new(target);

    // Connect to the target
    let conn = endpoint.connect(addr, TALOS_ALPN).await?;
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
    let buf = recv.read_to_end(64 * 1024).await?; // 64KB limit
    let response: Message = serde_json::from_slice(&buf)?;

    println!("🎉 Result Received: {:?}", response);
    Ok(())
}
