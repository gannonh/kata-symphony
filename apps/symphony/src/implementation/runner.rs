//! Local and Docker implementation worker invocation (PR1+).

use crate::error::Result;

/// Placeholder for credential-free implementation/repair runners.
#[derive(Debug, Clone, Default)]
pub struct ImplementationRunner;

impl ImplementationRunner {
    pub fn not_yet_implemented(&self) -> Result<()> {
        Err(crate::error::SymphonyError::StorageError(
            "implementation runner is not wired in PR1 foundation".to_string(),
        ))
    }
}
