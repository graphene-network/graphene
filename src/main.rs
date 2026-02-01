mod builder;
mod cache;
mod vmm;

use std::path::Path;
use std::path::PathBuf;
use vmm::{
    Virtualizer, firecracker::FirecrackerVirtualizer, mock::MockBehavior, mock::MockVirtualizer,
};

use builder::{DriveBuilder, mock::MockBuilder};
use cache::{DependencyCache, local::LocalDiskCache, mock::MockCache};

// We conditionally import LinuxBuilder only on Linux
#[cfg(target_os = "linux")]
use builder::linux::LinuxBuilder;

fn get_builder() -> Box<dyn DriveBuilder> {
    // Check if we are root and on Linux. If not, we MUST use mock for builder
    // because `mount` requires privileges.
    #[cfg(target_os = "linux")]
    {
        use nix::unistd::Uid;
        if Uid::effective().is_root() {
            println!("✅ Root detected. Using Real Linux Builder.");
            return Box::new(LinuxBuilder);
        }
    }

    println!("⚠️  Non-Root or Non-Linux detected. Using Mock Builder.");
    Box::new(MockBuilder)
}

// Factory Function
fn get_virtualizer() -> Box<dyn Virtualizer> {
    if Path::new("/dev/kvm").exists() {
        println!("✅ KVM Detected. Using Real Firecracker Engine.");
        Box::new(FirecrackerVirtualizer::new("/tmp/firecracker.sock"))
    } else {
        println!("⚠️  No KVM. Using Mock Engine (Happy Path).");
        Box::new(MockVirtualizer::new(MockBehavior::HappyPath))
    }
}

// The Factory Function
fn get_cache(use_mock: bool) -> Box<dyn DependencyCache> {
    if use_mock {
        println!("⚠️  Using Mock Cache (Memory Only)");
        Box::new(MockCache::new())
    } else {
        println!("✅ Using Local Disk Cache");
        Box::new(LocalDiskCache::new("./talos_cache"))
    }
}

async fn resolve_dependencies(
    cache: &Box<dyn DependencyCache>,
    builder: &dyn DriveBuilder, // From previous step
    job_id: &str,
    requirements: Vec<String>,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    // 1. Calculate Hash
    let hash = cache.calculate_hash(&requirements);

    // 2. Check Cache (Hot Path)
    if let Some(path) = cache.get(&hash).await? {
        return Ok(path);
    }

    // 3. Build It (Cold Path)
    println!("🧊 [RESOLVER] Cold Start triggered for Job {}", job_id);

    // Use the builder to create a temp image
    let temp_image = builder.build_dependency_drive(job_id, requirements).await?;

    // 4. Save to Cache
    let final_path = cache.put(&hash, temp_image).await?;

    Ok(final_path)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup Traits
    let mut vmm = get_virtualizer();
    let builder = get_builder();
    let cache = get_cache(true);

    // Mock Input
    let job_id = "job-101";
    let requirements = vec!["pandas".to_string(), "numpy".to_string()];

    println!("🔍 Resolving Dependencies...");
    let deps_drive_path = resolve_dependencies(&cache, &*builder, job_id, requirements).await?;

    println!("✅ Ready to Boot with Deps Drive: {:?}", deps_drive_path);

    // Boot VM (Mock or Real)
    vmm.configure(1, 128).await?;
    vmm.attach_drive("deps", deps_drive_path, false, true)
        .await?;
    vmm.start().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use builder::mock::MockBuilder;
    use cache::mock::MockCache;

    #[tokio::test]
    async fn test_cache_miss_triggers_build() {
        let cache = MockCache::new();
        let builder = MockBuilder; // Mock builder from previous step
        let job_id = "test-job";
        let reqs = vec!["pandas".to_string()];

        // 1. First Call: Should be a MISS
        // We know it's a miss because we haven't preloaded anything.
        // We expect the Resolver to call builder.build_dependency_drive
        let result1 = resolve_dependencies(&cache, &builder, job_id, reqs.clone()).await;
        assert!(result1.is_ok());

        // 2. Second Call: Should be a HIT
        // The first call should have called cache.put() internally.
        // So this time, it should return immediately without building.
        let result2 = resolve_dependencies(&cache, &builder, job_id, reqs).await;
        assert!(result2.is_ok());

        // (In a real test framework, you'd spy on the Builder to ensure it was called exactly once)
    }
}
