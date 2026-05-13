pub mod detectors;
pub mod input;
pub mod output;
pub mod streaming;
pub mod types;

pub use input::InputGuardrails;
pub use output::OutputGuardrails;
pub use streaming::{ViolationsObserver, wrap_stream_with_holdback};
pub use types::{GuardAction, GuardStage, GuardVerdict, Severity, Violation, ViolationCode};

use thairag_core::types::GuardrailViolationMeta;

/// Convert a list of internal `Violation`s into the wire-safe metadata records
/// stored in `PipelineMetadata`. Drops the matched substring — only codes,
/// severity, and stage cross this boundary.
pub fn violations_to_meta(violations: &[Violation]) -> Vec<GuardrailViolationMeta> {
    violations
        .iter()
        .map(|v| GuardrailViolationMeta {
            code: v.code.as_str().to_string(),
            severity: v.severity.as_str().to_string(),
            stage: v.stage.as_str().to_string(),
        })
        .collect()
}
