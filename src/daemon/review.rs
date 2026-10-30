use super::*;
use crate::domain::{Domain, Policy, ReviewDecision};

// This verifies the local operator interface, not a boundary against arbitrary same-UID code.
pub(super) fn operator_binding(stream: &UnixStream) -> Result<String> {
    let denied = || Error("REVIEW_OPERATOR_REQUIRED");
    let pid = stream.peer_cred()?.pid().ok_or_else(denied)?;
    let process = PathBuf::from(format!("/proc/{pid}"));
    let exe = fs::metadata(process.join("exe")).map_err(|_| denied())?;
    let own = fs::metadata("/proc/self/exe").map_err(|_| denied())?;
    if (exe.dev(), exe.ino()) != (own.dev(), own.ino()) {
        return Err(denied());
    }
    let stat = fs::read_to_string(process.join("stat")).map_err(|_| denied())?;
    let fields: Vec<_> = stat
        .rsplit_once(')')
        .ok_or_else(denied)?
        .1
        .split_whitespace()
        .collect();
    let tty: u64 = fields
        .get(4)
        .and_then(|s| s.parse::<i64>().ok())
        .map(|n| n as u32 as u64)
        .ok_or_else(denied)?;
    let input = fs::metadata(process.join("fd/0")).map_err(|_| denied())?;
    if tty == 0 || !input.file_type().is_char_device() || input.rdev() != tty {
        return Err(denied());
    }
    Ok(format!(
        "operator-cli/{pid}/{}",
        fields.get(19).ok_or_else(denied)?
    ))
}

impl Service {
    fn review_policy(&self) -> Policy {
        let snapshot = self.config.pin();
        Policy {
            revision: format!("review-cli/{}", snapshot.resolved().digest()),
            config_generation: snapshot.generation(),
            mode: snapshot.resolved().config().review.mode,
            scope: jobs::owner_scope(),
            adopt_explicit_decisions: false,
        }
    }

    pub(super) async fn review_preview(
        &self,
        decision: &ReviewDecision,
        id: &str,
    ) -> Result<serde_json::Value> {
        let _guard = self.admission.lock().await;
        Ok(serde_json::to_value(
            Domain::new(&self.storage)
                .preview_decision(&jobs::context()?, id, decision, &self.review_policy())
                .await?,
        )?)
    }

    pub(super) async fn review_apply(
        self: &Arc<Self>,
        decision: ReviewDecision,
        id: String,
        digest: String,
        binding: String,
    ) -> Result<serde_json::Value> {
        let service = self.clone();
        self.operation(async move {
            let _guard = service.admission.lock().await;
            let receipt = Domain::new(&service.storage)
                .apply_decision(
                    &jobs::context()?,
                    &id,
                    &decision,
                    &digest,
                    &binding,
                    &service.review_policy(),
                )
                .await?;
            service.wake_jobs();
            Ok(serde_json::to_value(receipt)?)
        })
        .await
    }
}
