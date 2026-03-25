use super::matrix::KernelMatrix;
use super::types::{KernelMetadata, KernelSpec};
use super::{KernelError, KernelRegistry};
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::fs;
use tokio::io::AsyncWriteExt;

/// Local filesystem-based kernel registry
///
/// Storage layout under `~/.opencapsule/kernels/`:
/// ```text
/// ~/.opencapsule/kernels/
/// ├── blobs/
/// │   └── <blake3-hash>           # Actual kernel binaries
/// ├── refs/
/// │   └── python-3.11-x86_64      # Symlinks to blobs
/// └── metadata/
///     └── python-3.11-x86_64.json # Kernel metadata
/// ```
pub struct LocalKernelRegistry {
    /// Base directory for kernel storage
    base_dir: PathBuf,
    /// Kernel matrix configuration
    matrix: KernelMatrix,
    /// Base URL for downloading kernels
    download_base_url: String,
}

impl LocalKernelRegistry {
    /// Create registry with default paths
    pub fn new(matrix: KernelMatrix) -> Result<Self, KernelError> {
        let base_dir = dirs::home_dir()
            .ok_or_else(|| KernelError::ConfigError("could not determine home directory".into()))?
            .join(".opencapsule")
            .join("kernels");

        Self::with_base_dir(base_dir, matrix)
    }

    /// Create registry with custom base directory
    pub fn with_base_dir(base_dir: PathBuf, matrix: KernelMatrix) -> Result<Self, KernelError> {
        Ok(Self {
            base_dir,
            matrix,
            download_base_url: "https://github.com/opencapsule/kernels/releases/download"
                .to_string(),
        })
    }

    /// Set custom download URL (useful for testing or private registries)
    pub fn with_download_url(mut self, url: impl Into<String>) -> Self {
        self.download_base_url = url.into();
        self
    }

    /// Initialize directory structure
    pub async fn init(&self) -> Result<(), KernelError> {
        fs::create_dir_all(self.blobs_dir()).await?;
        fs::create_dir_all(self.refs_dir()).await?;
        fs::create_dir_all(self.metadata_dir()).await?;
        Ok(())
    }

    fn blobs_dir(&self) -> PathBuf {
        self.base_dir.join("blobs")
    }

    fn refs_dir(&self) -> PathBuf {
        self.base_dir.join("refs")
    }

    fn metadata_dir(&self) -> PathBuf {
        self.base_dir.join("metadata")
    }

    fn ref_path(&self, spec: &KernelSpec) -> PathBuf {
        self.refs_dir().join(spec.canonical_name())
    }

    fn metadata_path(&self, spec: &KernelSpec) -> PathBuf {
        self.metadata_dir()
            .join(format!("{}.json", spec.canonical_name()))
    }

    fn blob_path(&self, hash: &str) -> PathBuf {
        self.blobs_dir().join(hash)
    }

    /// Download kernel from remote registry
    async fn download(&self, spec: &KernelSpec) -> Result<PathBuf, KernelError> {
        let url = format!(
            "{}/v{}/{}.unik",
            self.download_base_url,
            self.matrix.unikraft_version,
            spec.canonical_name()
        );

        tracing::info!("Downloading kernel from {}", url);

        let response = reqwest::get(&url).await?;

        if !response.status().is_success() {
            return Err(KernelError::NotFound(format!(
                "kernel {} not found at {} (status {})",
                spec.canonical_name(),
                url,
                response.status()
            )));
        }

        let bytes = response.bytes().await?;

        // Calculate hash
        let hash = blake3::hash(&bytes).to_hex().to_string();

        // Write to blobs directory
        let blob_path = self.blob_path(&hash);
        let mut file = fs::File::create(&blob_path).await?;
        file.write_all(&bytes).await?;
        file.flush().await?;

        // Create ref symlink
        let ref_path = self.ref_path(spec);
        if ref_path.exists() {
            fs::remove_file(&ref_path).await?;
        }

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&blob_path, &ref_path)?;
        }

        #[cfg(not(unix))]
        {
            // On non-Unix, just copy the file
            fs::copy(&blob_path, &ref_path).await?;
        }

        // Download and save metadata
        self.download_metadata(spec).await?;

        tracing::info!(
            "Downloaded kernel {} ({} bytes, hash {})",
            spec.canonical_name(),
            bytes.len(),
            &hash[..16]
        );

        Ok(ref_path)
    }

    /// Download kernel metadata
    async fn download_metadata(&self, spec: &KernelSpec) -> Result<KernelMetadata, KernelError> {
        let url = format!(
            "{}/v{}/{}.json",
            self.download_base_url,
            self.matrix.unikraft_version,
            spec.canonical_name()
        );

        let response = reqwest::get(&url).await?;

        if !response.status().is_success() {
            // Generate default metadata if not available
            return Ok(self.generate_default_metadata(spec));
        }

        let metadata: KernelMetadata = response.json().await.map_err(|e| {
            KernelError::ConfigError(format!("failed to parse metadata JSON: {}", e))
        })?;

        // Cache metadata locally
        let metadata_path = self.metadata_path(spec);
        let json = serde_json::to_string_pretty(&metadata).map_err(|e| {
            KernelError::ConfigError(format!("failed to serialize metadata: {}", e))
        })?;
        fs::write(&metadata_path, json).await?;

        Ok(metadata)
    }

    /// Generate default metadata from matrix configuration
    fn generate_default_metadata(&self, spec: &KernelSpec) -> KernelMetadata {
        let (min_memory, recommended_memory) = self.matrix.get_memory_config(&spec.runtime);
        let boot_args = self.matrix.get_boot_args(&spec.runtime);

        KernelMetadata {
            spec: spec.clone(),
            binary_hash: String::new(),
            binary_size_bytes: 0,
            min_memory_mib: min_memory,
            recommended_memory_mib: recommended_memory,
            default_boot_args: boot_args,
            unikraft_version: self.matrix.unikraft_version.clone(),
            built_at: None,
        }
    }

    /// Check if a kernel spec matches any in the matrix (with version fallback)
    fn find_matching_spec(&self, spec: &KernelSpec) -> Option<KernelSpec> {
        let available = self.matrix.all_specs();

        // Exact match first
        if available.iter().any(|s| s == spec) {
            return Some(spec.clone());
        }

        // Try version prefix match (e.g., "3.11.5" -> "3.11")
        let version_parts: Vec<&str> = spec.version.split('.').collect();
        if version_parts.len() > 2 {
            let minor_version = format!("{}.{}", version_parts[0], version_parts[1]);
            let fallback = KernelSpec {
                runtime: spec.runtime,
                version: minor_version,
                arch: spec.arch,
                variant: spec.variant.clone(),
            };
            if available.iter().any(|s| s == &fallback) {
                return Some(fallback);
            }
        }

        None
    }
}

#[async_trait]
impl KernelRegistry for LocalKernelRegistry {
    fn resolve(&self, name: &str) -> Result<KernelSpec, KernelError> {
        let spec = KernelSpec::parse(name).map_err(KernelError::InvalidSpec)?;

        // Verify it's in our matrix (with fallback)
        self.find_matching_spec(&spec)
            .ok_or_else(|| KernelError::NotFound(format!("kernel {} not in matrix", name)))
    }

    async fn get(&self, spec: &KernelSpec) -> Result<Option<PathBuf>, KernelError> {
        let ref_path = self.ref_path(spec);

        if ref_path.exists() {
            // Verify the blob still exists (symlink target)
            let target = fs::read_link(&ref_path).await.unwrap_or(ref_path.clone());
            if target.exists() {
                return Ok(Some(ref_path));
            }
            // Stale symlink, remove it
            let _ = fs::remove_file(&ref_path).await;
        }

        Ok(None)
    }

    async fn ensure(&self, spec: &KernelSpec) -> Result<PathBuf, KernelError> {
        // Resolve to canonical spec (with version fallback)
        let resolved = self
            .find_matching_spec(spec)
            .ok_or_else(|| KernelError::NotFound(spec.canonical_name()))?;

        // Check cache
        if let Some(path) = self.get(&resolved).await? {
            return Ok(path);
        }

        // Initialize directories if needed
        self.init().await?;

        // Download
        self.download(&resolved).await
    }

    fn list_available(&self) -> Vec<KernelSpec> {
        self.matrix.all_specs()
    }

    fn get_metadata(&self, spec: &KernelSpec) -> Result<KernelMetadata, KernelError> {
        let resolved = self
            .find_matching_spec(spec)
            .ok_or_else(|| KernelError::NotFound(spec.canonical_name()))?;

        // Try to load from cache synchronously (blocking)
        let metadata_path = self.metadata_path(&resolved);
        if metadata_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&metadata_path) {
                if let Ok(metadata) = serde_json::from_str(&content) {
                    return Ok(metadata);
                }
            }
        }

        // Fall back to generated defaults
        Ok(self.generate_default_metadata(&resolved))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_matrix() -> KernelMatrix {
        KernelMatrix::parse(
            r#"
unikraft_version = "0.17.0"

[defaults]
min_memory_mib = 128
recommended_memory_mib = 256

[runtimes.python]
versions = ["3.11", "3.12"]
architectures = ["x86_64"]
"#,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn test_init_creates_directories() {
        let temp = TempDir::new().unwrap();
        let registry =
            LocalKernelRegistry::with_base_dir(temp.path().to_path_buf(), test_matrix()).unwrap();

        registry.init().await.unwrap();

        assert!(temp.path().join("blobs").exists());
        assert!(temp.path().join("refs").exists());
        assert!(temp.path().join("metadata").exists());
    }

    #[test]
    fn test_resolve_valid_spec() {
        let temp = TempDir::new().unwrap();
        let registry =
            LocalKernelRegistry::with_base_dir(temp.path().to_path_buf(), test_matrix()).unwrap();

        let spec = registry.resolve("python-3.11").unwrap();
        assert_eq!(spec.version, "3.11");
    }

    #[test]
    fn test_resolve_invalid_spec() {
        let temp = TempDir::new().unwrap();
        let registry =
            LocalKernelRegistry::with_base_dir(temp.path().to_path_buf(), test_matrix()).unwrap();

        let result = registry.resolve("ruby-3.0");
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_version_fallback() {
        let temp = TempDir::new().unwrap();
        let registry =
            LocalKernelRegistry::with_base_dir(temp.path().to_path_buf(), test_matrix()).unwrap();

        // "3.11.5" should fall back to "3.11"
        let spec = registry.resolve("python-3.11.5").unwrap();
        assert_eq!(spec.version, "3.11");
    }

    #[tokio::test]
    async fn test_get_uncached_kernel() {
        let temp = TempDir::new().unwrap();
        let registry =
            LocalKernelRegistry::with_base_dir(temp.path().to_path_buf(), test_matrix()).unwrap();

        let spec = KernelSpec::parse("python-3.11").unwrap();
        let result = registry.get(&spec).await.unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_list_available() {
        let temp = TempDir::new().unwrap();
        let registry =
            LocalKernelRegistry::with_base_dir(temp.path().to_path_buf(), test_matrix()).unwrap();

        let available = registry.list_available();
        assert_eq!(available.len(), 2); // python 3.11, 3.12
    }

    #[test]
    fn test_get_metadata_defaults() {
        let temp = TempDir::new().unwrap();
        let registry =
            LocalKernelRegistry::with_base_dir(temp.path().to_path_buf(), test_matrix()).unwrap();

        let spec = KernelSpec::parse("python-3.11").unwrap();
        let metadata = registry.get_metadata(&spec).unwrap();

        assert_eq!(metadata.min_memory_mib, 128);
        assert_eq!(metadata.recommended_memory_mib, 256);
        assert_eq!(metadata.unikraft_version, "0.17.0");
    }
}
