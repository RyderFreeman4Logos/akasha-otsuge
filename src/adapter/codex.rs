//! Version-pinned native Codex integration.
mod transcript;
pub use transcript::reconcile_transcript;

pub const TRANSCRIPT_STREAM: &str = "codex-native-v1";
