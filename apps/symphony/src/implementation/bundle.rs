//! Content-addressed Git bundle create/verify/import (PR1+).

use crate::error::Result;

/// Placeholder for durable bundle blob operations.
#[derive(Debug, Clone, Default)]
pub struct BundleStore;

impl BundleStore {
    pub fn verify_placeholder(&self) -> Result<()> {
        Ok(())
    }
}
