//! Version-pinned native Codex integration.
mod transcript;
pub use transcript::reconcile_transcript;
mod caller;
pub use caller::{CodexCall, CodexCaller, CodexRegistration};

pub const TRANSCRIPT_STREAM: &str = "codex-native-v1";
