//! Get resource command

pub async fn run(
    _config_path: &str,
    node: &str,
    resource: &str,
    output_format: Option<&str>,
) -> anyhow::Result<()> {
    match resource {
        "config" => {
            println!("Getting configuration from node {}", node);
            // TODO: Implement
        }
        "status" => {
            println!("Getting status from node {}", node);
            // TODO: Implement
        }
        _ => {
            anyhow::bail!("Unknown resource: {}. Use 'config' or 'status'", resource);
        }
    }

    let format = output_format.unwrap_or("yaml");
    println!("(would output in {} format)", format);

    Ok(())
}
