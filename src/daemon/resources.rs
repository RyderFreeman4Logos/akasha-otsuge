use super::Service;
pub(super) use crate::resources::Resources;
use crate::resources::sample;
use serde_json::{Value, json};

impl Service {
    pub(super) async fn memory_available(&self) -> bool {
        let measured = self.resources.measure().await;
        let limits = self
            .config
            .pin()
            .resolved()
            .config()
            .runtime
            .memory_limits();
        self.resources.admits(measured.as_ref(), limits)
    }

    // Call while holding admission; routing estimates stay pinned, global capacity stays live.
    pub(super) async fn reserve_runner(
        &self,
        profile: &str,
        config: &crate::config::ResolvedConfig,
    ) -> super::Result<std::sync::Arc<crate::resources::Reservation>> {
        let estimate = config
            .config()
            .executors
            .get(profile)
            .and_then(|executor| executor.memory_estimate_mib)
            .unwrap_or_else(|| config.config().runtime.runner_estimate_mib());
        self.resources
            .reserve(
                self.config
                    .pin()
                    .resolved()
                    .config()
                    .runtime
                    .memory_limits(),
                profile,
                estimate,
            )
            .await
            .ok_or(super::Error("RESOURCE_BUSY"))
    }

    pub(super) async fn check_process_memory(&self) -> super::Result<()> {
        if self
            .resources
            .can_start(
                self.config
                    .pin()
                    .resolved()
                    .config()
                    .runtime
                    .memory_limits(),
            )
            .await
        {
            Ok(())
        } else {
            Err(super::Error("RESOURCE_BUSY"))
        }
    }

    pub(super) fn storage_reopen_required(&self) -> bool {
        self.storage.budget_mib()
            != self
                .config
                .pin()
                .resolved()
                .config()
                .runtime
                .memory_budget_mib
    }

    pub(super) async fn resource_status(&self) -> Value {
        let config = self.config.pin();
        let requested = config.resolved().config().runtime.memory_limits();
        let index = match self.search_index.try_lock() {
            Ok(index) => {
                json!({"inspected":true,"open_budget_mib":index.as_ref().map(|i|i.budget_mib)})
            }
            Err(_) => json!({"inspected":false,"open_budget_mib":null}),
        };
        let measurement = self.resources.measure().await;
        let draining = self.storage.budget_mib() != requested.daemon_memory_target_mib
            || !self.resources.admits(measurement.as_ref(), requested);
        json!({"config_generation":config.generation(),"requested":requested,
            "storage_open_budget_mib":self.storage.budget_mib(),
            "storage_reopen_required":self.storage.budget_mib()!=requested.daemon_memory_target_mib,
            "draining":draining,
            "index":index,"measurement":sample(measurement),"workers":self.resources.status(),
            "enforcement":{"software_memory_admission":true,"managed_cgroup_limits":false}})
    }
}
