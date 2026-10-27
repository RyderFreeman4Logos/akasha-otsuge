use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::watch;

use super::{ConfigError, Notifier, Paths, ResolvedConfig, resolve};

pub struct Snapshot<R> {
    generation: u64,
    resolved: ResolvedConfig,
    resources: R,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_durable_commit_keeps_active_pending_history_and_watch_unchanged() {
        let paths = Paths {
            data_dir: "/test/data".into(),
            socket_path: "/test/run/socket".into(),
        };
        let manager =
            ConfigManager::new(1, resolve("", None, "", &paths).unwrap(), |_| Ok(())).unwrap();
        manager
            .reload("[paths]\ndata_dir='/test/next'", None, |_| Ok(()))
            .unwrap();
        let before = manager.checkpoint().unwrap();
        let rx = manager.subscribe();
        for input in [
            "[timeouts]\nrunner='2100s'",
            "[paths]\ndata_dir='/test/other'",
            "[broken",
        ] {
            let error = manager
                .reload_committing(input, None, |_| Ok(()), |_| Err(()))
                .unwrap_err();
            assert_eq!(error.field, "commit");
            assert_eq!(manager.checkpoint().unwrap(), before);
            assert!(!rx.has_changed().unwrap());
        }
        let restored = ConfigManager::restore(before.clone(), |_| Ok(())).unwrap();
        assert_eq!(restored.checkpoint().unwrap(), before);
    }
}

impl<R> Snapshot<R> {
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn resolved(&self) -> &ResolvedConfig {
        &self.resolved
    }
    pub fn resources(&self) -> &R {
        &self.resources
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReloadStatus {
    Applied,
    Rejected,
    RestartRequired,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReloadReport {
    pub status: ReloadStatus,
    pub active_generation: u64,
    pub pending_generation: Option<u64>,
    pub candidate_digest: Option<String>,
    pub reasons: Vec<ConfigError>,
}

#[derive(Clone)]
struct State {
    pending: Option<(u64, ResolvedConfig)>,
    history: VecDeque<Value>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    schema_version: u32,
    generation: u64,
    active: super::ArchivedConfig,
    pending: Option<(u64, super::ArchivedConfig)>,
    history: VecDeque<Value>,
}

fn checkpoint(generation: u64, active: &ResolvedConfig, state: &State) -> Value {
    serde_json::to_value(Checkpoint {
        schema_version: 1,
        generation,
        active: active.archive(),
        pending: state.pending.as_ref().map(|(g, c)| (*g, c.archive())),
        history: state.history.clone(),
    })
    .expect("JSON-compatible configuration")
}

/// R holds resources prepared for exactly one generation; pinned jobs retain them.
pub struct ConfigManager<R> {
    state: Mutex<State>,
    snapshots: watch::Sender<Arc<Snapshot<R>>>,
    paths: Paths,
}

impl<R: Send + Sync + 'static> ConfigManager<R> {
    /// The daemon must restore the generation from durable state on restart.
    pub fn new(
        generation: u64,
        initial: ResolvedConfig,
        prepare: impl FnOnce(&ResolvedConfig) -> Result<R, ()>,
    ) -> Result<Self, ConfigError> {
        if generation == 0 {
            return Err(ConfigError::invalid(
                "generation",
                "generation must be positive",
            ));
        }
        let resources = prepare(&initial)
            .map_err(|_| ConfigError::invalid("prepare", "initial resource preparation failed"))?;
        let paths = initial.base_paths.clone();
        let (snapshots, _) = watch::channel(Arc::new(Snapshot {
            generation,
            resolved: initial,
            resources,
        }));
        Ok(Self {
            state: Mutex::new(State {
                pending: None,
                history: VecDeque::new(),
            }),
            snapshots,
            paths,
        })
    }

    pub fn pin(&self) -> Arc<Snapshot<R>> {
        Arc::clone(&self.snapshots.borrow())
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<Snapshot<R>>> {
        self.snapshots.subscribe()
    }

    pub(crate) fn checkpoint(&self) -> Result<Value, ConfigError> {
        let state = self
            .state
            .lock()
            .map_err(|_| ConfigError::invalid("reload", "configuration manager unavailable"))?;
        let active = self.pin();
        Ok(checkpoint(active.generation, &active.resolved, &state))
    }

    pub(crate) fn restore(
        value: Value,
        prepare: impl FnOnce(&ResolvedConfig) -> Result<R, ()>,
    ) -> Result<Self, ConfigError> {
        let saved: Checkpoint = serde_json::from_value(value)
            .map_err(|_| ConfigError::invalid("generation", "invalid durable configuration"))?;
        if saved.schema_version != 1
            || saved.history.len() > 64
            || saved
                .pending
                .as_ref()
                .is_some_and(|(g, _)| Some(*g) != saved.generation.checked_add(1))
        {
            return Err(ConfigError::invalid(
                "generation",
                "invalid durable configuration",
            ));
        }
        let manager = Self::new(
            saved.generation,
            ResolvedConfig::restore(saved.active)?,
            prepare,
        )?;
        {
            let mut state = manager
                .state
                .lock()
                .map_err(|_| ConfigError::invalid("reload", "configuration manager unavailable"))?;
            state.pending = saved
                .pending
                .map(|(g, c)| ResolvedConfig::restore(c).map(|c| (g, c)))
                .transpose()?;
            state.history = saved.history;
        }
        Ok(manager)
    }

    /// Startup owns the requested paths; persist this manager before serving requests.
    pub(crate) fn restarted(
        self,
        initial: ResolvedConfig,
        prepare: impl FnOnce(&ResolvedConfig) -> Result<R, ()>,
    ) -> Result<Self, ConfigError> {
        let active = self.pin();
        if initial.digest == active.resolved.digest
            && initial.base_paths == active.resolved.base_paths
            && initial.selected_profile == active.resolved.selected_profile
            && initial.sources == active.resolved.sources
        {
            return Ok(self);
        }
        let next = active.generation.checked_add(1).ok_or_else(|| {
            ConfigError::invalid("generation", "configuration generation exhausted")
        })?;
        let report = ReloadReport {
            status: ReloadStatus::Applied,
            active_generation: next,
            pending_generation: None,
            candidate_digest: Some(initial.digest().into()),
            reasons: Vec::new(),
        };
        let manager = Self::new(next, initial, prepare)?;
        let mut state = self
            .state
            .into_inner()
            .map_err(|_| ConfigError::invalid("reload", "configuration manager unavailable"))?;
        state.pending = None;
        if state.history.len() == 64 {
            state.history.pop_front();
        }
        state
            .history
            .push_back(serde_json::to_value(report).expect("JSON-compatible report"));
        *manager.state.lock().expect("new manager") = state;
        Ok(manager)
    }

    /// File watching and explicit reload both submit complete bytes here.
    /// A prepare callback must not re-enter this manager or publish partial resources.
    pub fn reload(
        &self,
        input: &str,
        profile: Option<&str>,
        prepare: impl FnOnce(&ResolvedConfig) -> Result<R, ()>,
    ) -> Result<ReloadReport, ConfigError> {
        self.reload_committing(input, profile, prepare, |_| Ok(()))
    }

    /// Persist the complete candidate before either active or pending state is visible.
    /// The callback must not re-enter this manager. Daemon calls this off the async thread.
    pub(crate) fn reload_committing(
        &self,
        input: &str,
        profile: Option<&str>,
        prepare: impl FnOnce(&ResolvedConfig) -> Result<R, ()>,
        persist: impl FnOnce(&Value) -> Result<(), ()>,
    ) -> Result<ReloadReport, ConfigError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ConfigError::invalid("reload", "configuration manager unavailable"))?;
        let active = self.pin();
        let mut candidate_state = state.clone();
        let mut replacement = None;
        let mut report = ReloadReport {
            status: ReloadStatus::Rejected,
            active_generation: active.generation,
            pending_generation: state.pending.as_ref().map(|(g, _)| *g),
            candidate_digest: None,
            reasons: Vec::new(),
        };
        match resolve(input, profile, "", &self.paths) {
            Err(error) => report.reasons.push(error),
            Ok(candidate) => {
                report.candidate_digest = Some(candidate.digest().into());
                let next = active.generation.checked_add(1).ok_or_else(|| {
                    ConfigError::invalid("generation", "configuration generation exhausted")
                })?;
                if candidate.config().paths != active.resolved.config().paths {
                    report.status = ReloadStatus::RestartRequired;
                    report.pending_generation = Some(next);
                    report.reasons.push(ConfigError {
                        code: "CONFIG_RESTART_REQUIRED", field: "paths".into(),
                        message: "data directory or socket change requires explicit drain and restart",
                        byte_offset: None,
                    });
                    candidate_state.pending = Some((next, candidate));
                } else {
                    match prepare(&candidate) {
                        Err(()) => report.reasons.push(ConfigError::invalid(
                            "prepare",
                            "resource preparation failed; old configuration retained",
                        )),
                        Ok(resources) => {
                            replacement = Some(Arc::new(Snapshot {
                                generation: next,
                                resolved: candidate,
                                resources,
                            }));
                            candidate_state.pending = None;
                            report.status = ReloadStatus::Applied;
                            report.active_generation = next;
                            report.pending_generation = None;
                        }
                    }
                }
            }
        }
        if candidate_state.history.len() == 64 {
            candidate_state.history.pop_front();
        }
        candidate_state
            .history
            .push_back(serde_json::to_value(&report).expect("JSON-compatible report"));
        let planned = replacement.as_ref().unwrap_or(&active);
        persist(&checkpoint(
            planned.generation,
            &planned.resolved,
            &candidate_state,
        ))
        .map_err(|_| {
            ConfigError::invalid(
                "commit",
                "durable configuration commit failed; old configuration retained",
            )
        })?;
        *state = candidate_state;
        if let Some(replacement) = replacement {
            self.snapshots.send_replace(replacement);
        }
        Ok(report)
    }

    /// Read live for each dispatch/retry, even when the job pins an old generation.
    pub fn delivery_route(&self, channel: &str) -> Option<(u64, Notifier)> {
        let current = self.pin();
        current
            .resolved
            .config()
            .notifiers
            .get(channel)
            .filter(|n| n.enabled)
            .map(|route| (current.generation, route.clone()))
    }

    pub fn executor_allowed(&self, name: &str) -> bool {
        let current = self.pin();
        !current.resolved.config().runtime.paused
            && current
                .resolved
                .config()
                .executors
                .get(name)
                .is_some_and(|e| e.enabled)
    }

    pub fn status(&self) -> Result<Value, ConfigError> {
        let state = self
            .state
            .lock()
            .map_err(|_| ConfigError::invalid("reload", "configuration manager unavailable"))?;
        let active = self.pin();
        Ok(json!({
            "active_generation": active.generation,
            "effective": active.resolved.diagnostic(),
            "pending": state.pending.as_ref().map(|(g, c)| json!({ "generation": g, "desired": c.diagnostic() })),
            "revision_log": state.history,
        }))
    }
}
