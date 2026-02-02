use monad_node::{protocol, vmm};
use anyhow::Result;
use iroh::SecretKey;
use iroh::endpoint::Endpoint;
use protocol::{Message, TALOS_ALPN};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use rand::rngs::OsRng;

// Import your existing Mock/Real logic
use vmm::{Virtualizer, mock::{MockVirtualizer, MockBehavior}};

#[tokio::main]
async fn main() -> Result<()> {
    println!("🦀 Talos P2P Node Initializing...");

    // 1. Generate Identity (The "Wallet" of the networking layer)
    // In prod, load this from disk so your PeerID stays the same.
    let mut rng = OsRng;
    let secret_key = SecretKey::generate(&mut rng);
    println!("🆔 My Peer ID: {}", secret_key.public());

    // 2. Bind to a UDP port (Iroh uses QUIC/UDP)
    // 0 means "random available port"
    let endpoint = Endpoint::builder()
        .secret_key(secret_key)
        .alpns(vec![TALOS_ALPN.to_vec()])
        .bind()
        .await?;

    // Print the "Ticket" (How others find us)
    // This combines IP + Port + PeerID into one string.
    let addr = endpoint.local_addr()?;
    println!("🌍 Listening on: {:?}", addr);
    // println!("🎟️  Ticket to Connect: {}", my_addr.node_id);
    // (In reality, you'd print the full NodeAddr ticket string here)

    // 3. Start the VMM Engine (Shared State)
    let vmm = Arc::new(tokio::sync::Mutex::new(MockVirtualizer::new(MockBehavior::HappyPath)));

    // 4. Accept Incoming Connections (The "Server" Loop)
    while let Some(incoming) = endpoint.accept().await {
        let vmm_clone = vmm.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(incoming, vmm_clone).await {
                eprintln!("❌ Connection Error: {:?}", e);
            }
        });
    }

    Ok(())
}

/// Handle a new peer connecting to us
async fn handle_connection(
    mut incoming: iroh::endpoint::Incoming,
    vmm: Arc<tokio::sync::Mutex<impl Virtualizer>>,
) -> Result<()> {
    // A. Perform the TLS Handshake
    let connecting = incoming.accept()?;
    let connection = connecting.await?;
    let remote_peer = connection.remote_id()?;
    println!("🔗 Connected to Peer: {}", remote_peer);

    // B. Accept a bi-directional stream (like a TCP socket)
    let (mut send, mut recv) = connection.accept_bi().await?;

    // C. Read the Request (JSON over QUIC)
    let mut buf = vec![0u8; 4096]; // Buffer for message
    
    // Read returns Result<Option<usize>>
    let n = match recv.read(&mut buf).await? {
        Some(n) => n,
        None => return Ok(()),
    };

    // TODO: use rkyv or bincode instead
    let msg: Message = serde_json::from_slice(&buf[..n])?;

    match msg {
        Message::JobRequest {
            job_id,
            code,
            requirements,
        } => {
            println!("📩 Received Job: {} | Reqs: {:?}", job_id, requirements);

            // D. TRIGGER THE JIT VMM (Your Logic)
            println!("    ⚙️ Spinning up MockVMM...");
            {
                let mut machine = vmm.lock().await;
                // In real code: builder.build(reqs)...
                machine.start().await.unwrap();
                machine.wait().await.unwrap();
            }

            // E. Send Response
            let response = Message::JobResult {
                job_id,
                output: "Hello from Talos P2P!".to_string(),
                status: "Success".to_string(),
            };

            let resp_bytes = serde_json::to_vec(&response)?;
            send.write_all(&resp_bytes).await?;
            send.finish()?;
        }
        _ => println!("⚠️ Unknown message type"),
    }

    Ok(())
}