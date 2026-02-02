use crate::unikraft::{
    BuildJob, BuildManifest, Kraftfile, UnikernelBuilder, UnikernelImage, UnikraftError,
    ValidatedDockerfile,
};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// Configurable behaviors for the mock builder
#[derive(Clone, Debug, Default)]
pub enum MockBuildBehavior {
    /// Build succeeds with a valid unikernel
    #[default]
    HappyPath,
    /// Build times out after specified duration
    BuildTimeout(Duration),
    /// Build fails with specified exit code and error message
    BuildFailure { exit_code: i32, stderr: String },
    /// Dockerfile validation fails
    ValidationError(String),
}

/// Spy state for tracking mock builder calls
#[derive(Default, Debug)]
pub struct BuilderSpyState {
    /// All build jobs that were submitted
    pub build_calls: Vec<BuildJob>,
    /// All Dockerfiles that were validated
    pub validate_calls: Vec<String>,
    /// All manifests used for Kraftfile generation
    pub kraftfile_calls: Vec<(BuildManifest, String)>,
}

/// Mock implementation of UnikernelBuilder for testing
#[derive(Clone)]
pub struct MockKraftBuilder {
    /// Spy state shared with test code
    pub spy: Arc<Mutex<BuilderSpyState>>,
    /// Configured behavior for the mock
    pub behavior: MockBuildBehavior,
    /// Validator for Dockerfiles (reuse real validation logic)
    validator: Arc<crate::unikraft::dockerfile::DockerfileValidator>,
}

impl Default for MockKraftBuilder {
    fn default() -> Self {
        Self::new(MockBuildBehavior::HappyPath)
    }
}

impl MockKraftBuilder {
    /// Create a new mock builder with specified behavior
    pub fn new(behavior: MockBuildBehavior) -> Self {
        Self {
            spy: Arc::new(Mutex::new(BuilderSpyState::default())),
            behavior,
            validator: Arc::new(crate::unikraft::dockerfile::DockerfileValidator::new()),
        }
    }

    /// Create a happy-path mock builder
    pub fn happy_path() -> Self {
        Self::new(MockBuildBehavior::HappyPath)
    }

    /// Create a mock that simulates build timeout
    pub fn timeout(duration: Duration) -> Self {
        Self::new(MockBuildBehavior::BuildTimeout(duration))
    }

    /// Create a mock that simulates build failure
    pub fn failure(exit_code: i32, stderr: impl Into<String>) -> Self {
        Self::new(MockBuildBehavior::BuildFailure {
            exit_code,
            stderr: stderr.into(),
        })
    }

    /// Create a mock that simulates validation failure
    pub fn validation_error(message: impl Into<String>) -> Self {
        Self::new(MockBuildBehavior::ValidationError(message.into()))
    }

    // --- Spy Methods ---

    /// Get the number of build calls made
    pub fn build_count(&self) -> usize {
        self.spy.lock().unwrap().build_calls.len()
    }

    /// Get the number of validation calls made
    pub fn validate_count(&self) -> usize {
        self.spy.lock().unwrap().validate_calls.len()
    }

    /// Check if a specific job ID was built
    pub fn was_job_built(&self, job_id: &str) -> bool {
        self.spy
            .lock()
            .unwrap()
            .build_calls
            .iter()
            .any(|job| job.job_id == job_id)
    }

    /// Get all job IDs that were built
    pub fn built_job_ids(&self) -> Vec<String> {
        self.spy
            .lock()
            .unwrap()
            .build_calls
            .iter()
            .map(|job| job.job_id.clone())
            .collect()
    }

    /// Get the last build job submitted
    pub fn last_build_job(&self) -> Option<BuildJob> {
        self.spy.lock().unwrap().build_calls.last().cloned()
    }
}

#[async_trait]
impl UnikernelBuilder for MockKraftBuilder {
    async fn build(&self, job: &BuildJob) -> Result<UnikernelImage, UnikraftError> {
        // Record the call
        self.spy.lock().unwrap().build_calls.push(job.clone());

        // Check behavior
        match &self.behavior {
            MockBuildBehavior::HappyPath => {
                // Validate first (like real implementation)
                self.validate_dockerfile(&job.dockerfile)?;

                // Generate mock output
                let hash = blake3::hash(job.dockerfile.as_bytes());
                let mock_path = std::env::temp_dir()
                    .join("graphene-mock-unikraft")
                    .join(format!("{}.unik", job.job_id));

                // Create the mock file
                std::fs::create_dir_all(mock_path.parent().unwrap())?;
                std::fs::write(&mock_path, format!("MOCK_UNIKERNEL_{}", job.job_id))?;

                Ok(UnikernelImage {
                    hash: *hash.as_bytes(),
                    path: mock_path,
                    size_bytes: 1024, // Mock size
                    runtime: job.manifest.runtime,
                    built_at: SystemTime::now(),
                })
            }
            MockBuildBehavior::BuildTimeout(duration) => {
                // Simulate timeout by sleeping then returning error
                tokio::time::sleep(*duration).await;
                Err(UnikraftError::BuildTimeout {
                    elapsed: *duration,
                    limit: *duration,
                })
            }
            MockBuildBehavior::BuildFailure { exit_code, stderr } => {
                Err(UnikraftError::BuildFailed {
                    exit_code: *exit_code,
                    stderr: stderr.clone(),
                })
            }
            MockBuildBehavior::ValidationError(msg) => {
                Err(UnikraftError::DockerfileParseError(msg.clone()))
            }
        }
    }

    fn validate_dockerfile(&self, dockerfile: &str) -> Result<ValidatedDockerfile, UnikraftError> {
        // Record the call
        self.spy
            .lock()
            .unwrap()
            .validate_calls
            .push(dockerfile.to_string());

        // Check for validation error behavior
        if let MockBuildBehavior::ValidationError(msg) = &self.behavior {
            return Err(UnikraftError::DockerfileParseError(msg.clone()));
        }

        // Use real validation logic
        self.validator.validate(dockerfile)
    }

    fn generate_kraftfile(&self, manifest: &BuildManifest, name: &str) -> Kraftfile {
        // Record the call
        self.spy
            .lock()
            .unwrap()
            .kraftfile_calls
            .push((manifest.clone(), name.to_string()));

        // Return real Kraftfile
        Kraftfile::from_manifest(manifest, name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unikraft::{ResourceLimits, Runtime};

    fn sample_job() -> BuildJob {
        BuildJob {
            job_id: "test-123".to_string(),
            dockerfile: r#"
FROM graphene/node:20
WORKDIR /app
COPY . .
RUN npm install
CMD ["node", "index.js"]
"#
            .to_string(),
            source_code: vec![],
            manifest: BuildManifest {
                runtime: Runtime::Node20,
                entrypoint: vec!["node".to_string(), "index.js".to_string()],
                resources: ResourceLimits::default(),
            },
        }
    }

    #[tokio::test]
    async fn test_mock_happy_path() {
        let builder = MockKraftBuilder::happy_path();
        let job = sample_job();

        let result = builder.build(&job).await;
        assert!(result.is_ok());

        let image = result.unwrap();
        assert!(image.path.exists());
        assert!(image.size_bytes > 0);

        // Verify spy recorded the call
        assert_eq!(builder.build_count(), 1);
        assert!(builder.was_job_built("test-123"));
    }

    #[tokio::test]
    async fn test_mock_build_failure() {
        let builder = MockKraftBuilder::failure(1, "kraft: command not found");
        let job = sample_job();

        let result = builder.build(&job).await;
        assert!(matches!(
            result,
            Err(UnikraftError::BuildFailed { exit_code: 1, .. })
        ));

        // Spy should still record the attempt
        assert_eq!(builder.build_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_validation_error() {
        let builder = MockKraftBuilder::validation_error("Invalid Dockerfile");
        let job = sample_job();

        let result = builder.build(&job).await;
        assert!(matches!(
            result,
            Err(UnikraftError::DockerfileParseError(_))
        ));
    }

    #[tokio::test]
    async fn test_mock_timeout() {
        let builder = MockKraftBuilder::timeout(Duration::from_millis(10));
        let job = sample_job();

        let start = std::time::Instant::now();
        let result = builder.build(&job).await;
        let elapsed = start.elapsed();

        assert!(matches!(result, Err(UnikraftError::BuildTimeout { .. })));
        assert!(elapsed >= Duration::from_millis(10));
    }

    #[test]
    fn test_spy_tracks_validation_calls() {
        let builder = MockKraftBuilder::happy_path();
        let dockerfile = "FROM graphene/node:20\nCMD [\"node\", \"index.js\"]";

        let _ = builder.validate_dockerfile(dockerfile);

        assert_eq!(builder.validate_count(), 1);
    }

    #[test]
    fn test_spy_tracks_kraftfile_generation() {
        let builder = MockKraftBuilder::happy_path();
        let manifest = BuildManifest {
            runtime: Runtime::Node20,
            entrypoint: vec!["node".to_string()],
            resources: ResourceLimits::default(),
        };

        let _ = builder.generate_kraftfile(&manifest, "test-app");

        let spy = builder.spy.lock().unwrap();
        assert_eq!(spy.kraftfile_calls.len(), 1);
        assert_eq!(spy.kraftfile_calls[0].1, "test-app");
    }
}
