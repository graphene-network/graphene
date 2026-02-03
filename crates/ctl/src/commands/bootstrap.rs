//! Bootstrap command - connect to a new node and save credentials

pub async fn run(nodes: Vec<String>, output: String) -> anyhow::Result<()> {
    println!("Bootstrapping connection to nodes: {:?}", nodes);
    println!("Output will be saved to: {}", output);

    // TODO: Implement actual bootstrap:
    // 1. Connect to node via Iroh
    // 2. Perform initial handshake
    // 3. Retrieve admin capability token
    // 4. Save to config file

    anyhow::bail!("Bootstrap not yet implemented. Use cloud-init or console access to get initial credentials.")
}
