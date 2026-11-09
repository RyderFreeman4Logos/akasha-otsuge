use super::*;
use crate::domain::{Domain, Generator, Ingress, MemoryPatch, Origin, Policy};

impl Service {
    pub(super) async fn apply_patch(
        self: &Arc<Self>,
        patch: MemoryPatch,
        id: String,
    ) -> Result<serde_json::Value> {
        let service = self.clone();
        self.operation(async move {
            let _guard = service.admission.lock().await;
            let config = service.config.pin();
            let ingress = Ingress::agent(
                "external-editor",
                &format!("patch-cli/{}", crate::storage::hash(id.as_bytes())),
                patch.scope.clone(),
                Origin::MemoryEdit,
                Generator {
                    runner: "external-cli".into(),
                    model: "unspecified".into(),
                    prompt_version: "memory-patch-v1".into(),
                },
            )?;
            let policy = Policy {
                revision: format!("memory-patch/{}", config.resolved().digest()),
                config_generation: config.generation(),
                mode: config.resolved().config().review.mode,
                scope: patch.scope.clone(),
                adopt_explicit_decisions: false,
            };
            let receipt = Domain::new(&service.storage)
                .apply_patch(
                    &jobs::context()?,
                    &id,
                    &patch,
                    &ingress,
                    &policy,
                    ipc::now_ms()?,
                )
                .await?;
            service.wake_jobs();
            Ok(serde_json::to_value(receipt)?)
        })
        .await
    }
}
