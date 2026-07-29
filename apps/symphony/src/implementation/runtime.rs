//! Startup and poll-loop integration helpers for the implementation stage.

use crate::implementation::domain::ImplementationConfig;
use sha2::{Digest, Sha256};

/// Hash the durable implementation configuration revision.
///
/// Includes schema version, both prompt *contents*, model, timeouts,
/// attempt/cycle/bundle bounds, spec template, validation commands, and
/// completion route — matching the A3 design contract.
pub fn implementation_configuration_revision(
    config: &ImplementationConfig,
    prompt: &str,
    repair_prompt: &str,
) -> String {
    let value = serde_json::json!({
        "schema_version": 1,
        "prompts": {
            "implement": prompt,
            "repair": repair_prompt,
        },
        "model": config.model,
        "max_turns": config.max_turns,
        "invocation_timeout_ms": config.invocation_timeout_ms,
        "max_attempts": config.max_attempts,
        "max_validation_cycles": config.max_validation_cycles,
        "max_bundle_bytes": config.max_bundle_bytes,
        "spec_file": config.spec_file,
        "validation": config.validation,
        "completion_route": config.completion_route,
        "mode": config.mode.as_str(),
    });
    let bytes = serde_json::to_vec(&value).expect("configuration revision JSON serializes");
    hex::encode(Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::implementation::domain::ImplementationConfig;

    #[test]
    fn configuration_revision_is_stable_for_identical_inputs() {
        let config = ImplementationConfig::default();
        let one = implementation_configuration_revision(&config, "a\nb", "r");
        let two = implementation_configuration_revision(&config, "a\nb", "r");
        assert_eq!(one, two);
        assert_ne!(
            one,
            implementation_configuration_revision(&config, "a\nb", "changed")
        );
    }
}
