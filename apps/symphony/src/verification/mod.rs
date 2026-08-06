//! A5 verification stage: exact-head command execution, evidence capture,
//! read-only verifier invocation, and the deterministic gate.

pub mod coordinator;
pub mod domain;
pub mod evidence;
pub mod executor;
pub mod gate;
pub mod publisher;
pub mod worker;
pub mod workspace;
