use super::{BuilderError, DriveBuilder};
use async_trait::async_trait;
use std::env;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// The "Spy State" - shared between the Mock and your Test
#[derive(Default, Debug)]
pub struct BuilderSpyState {
    pub create_code_calls: Vec<String>,     // Log of job_ids
    pub build_deps_calls: Vec<Vec<String>>, // Log of package lists requested
}

#[derive(Clone)]
pub struct MockBuilder {
    pub spy: Arc<Mutex<BuilderSpyState>>,
}

impl Default for MockBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl MockBuilder {
    pub fn new() -> Self {
        Self {
            spy: Arc::new(Mutex::new(BuilderSpyState::default())),
        }
    }

    // --- SPY METHODS (Only available on MockBuilder, not the Trait) ---

    pub fn get_code_build_count(&self) -> usize {
        self.spy.lock().unwrap().create_code_calls.len()
    }

    pub fn get_deps_build_count(&self) -> usize {
        self.spy.lock().unwrap().build_deps_calls.len()
    }

    pub fn was_package_requested(&self, pkg: &str) -> bool {
        let state = self.spy.lock().unwrap();
        for reqs in &state.build_deps_calls {
            if reqs.contains(&pkg.to_string()) {
                return true;
            }
        }
        false
    }
}

#[async_trait]
impl DriveBuilder for MockBuilder {
    async fn create_code_drive(
        &self,
        job_id: &str,
        _content: &str,
    ) -> Result<PathBuf, BuilderError> {
        // Record the call
        self.spy
            .lock()
            .unwrap()
            .create_code_calls
            .push(job_id.to_string());

        // Return dummy path
        let mut path = env::temp_dir();
        path.push(format!("mock_code_{}.img", job_id));
        Ok(path)
    }

    async fn build_dependency_drive(
        &self,
        _job_id: &str,
        packages: Vec<String>,
    ) -> Result<PathBuf, BuilderError> {
        // Record the call
        self.spy
            .lock()
            .unwrap()
            .build_deps_calls
            .push(packages.clone());

        let mut path = env::temp_dir();
        path.push("mock_deps.img");
        Ok(path)
    }
}
