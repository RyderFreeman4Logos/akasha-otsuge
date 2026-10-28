use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::Url;

use super::{ConfigError, deadline::duration};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Paths {
    pub data_dir: PathBuf,
    pub socket_path: PathBuf,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub paths: Paths,
    pub consult: Consult,
    pub timeouts: Timeouts,
    pub runtime: Runtime,
    pub retrieval: Retrieval,
    pub review: Review,
    pub access: Access,
    pub logging: Logging,
    #[serde(default)]
    pub executors: BTreeMap<String, Executor>,
    #[serde(default)]
    pub models: BTreeMap<String, Model>,
    #[serde(default)]
    pub notifiers: BTreeMap<String, Notifier>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Consult {
    pub executor: String,
    pub max_runner_starts: u32,
    pub max_delegation_depth: u32,
    pub max_result_chars: usize,
    pub instructions_version: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timeouts {
    pub local_request: String,
    pub runner: String,
    pub workflow: String,
    pub idle: String,
    pub queue: String,
    pub startup: String,
    pub cleanup: String,
    pub tools: String,
    pub summary: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Runtime {
    pub max_concurrent_jobs: usize,
    pub max_queued_jobs: usize,
    pub memory_budget_mib: u32,
    pub paused: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Retrieval {
    pub top_k: usize,
    pub max_candidates: usize,
    pub max_original_chars: usize,
    #[serde(default)]
    pub embedding: Option<String>,
    #[serde(default)]
    pub reranker: Option<String>,
    #[serde(default)]
    pub extraction: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub mode: ReviewMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewMode {
    Manual,
    Policy,
    Automatic,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Access {
    pub read_roots: Vec<PathBuf>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Logging {
    pub level: LogLevel,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

fn enabled() -> bool {
    true
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Executor {
    pub kind: Host,
    pub binary: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub timeout: Option<String>,
    #[serde(default)]
    pub fallback: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Host {
    ClaudeCode,
    Codex,
    Hermes,
    Pi,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub kind: ModelKind,
    pub endpoint: Url,
    pub model: String,
    pub local: bool,
    #[serde(default)]
    pub credential_ref: Option<String>,
    #[serde(default)]
    pub timeout: Option<String>,
    #[serde(default)]
    pub dimension: Option<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelKind {
    Text,
    Embedding,
    Reranker,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Notifier {
    pub kind: NotifierKind,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub credential_ref: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifierKind {
    Local,
    Stdio,
}

#[derive(Clone, Debug, Serialize)]
pub struct EffectiveTimeouts {
    pub local_request: Duration,
    pub runner: Duration,
    pub workflow: Duration,
    pub idle: Option<Duration>,
    pub queue: Duration,
    pub startup: Duration,
    pub cleanup: Duration,
    pub tools: Duration,
    pub summary: Duration,
    pub providers: BTreeMap<String, Duration>,
    pub executors: BTreeMap<String, Duration>,
}

fn require(ok: bool, field: &str, message: &'static str) -> Result<(), ConfigError> {
    if ok {
        Ok(())
    } else {
        Err(ConfigError::invalid(field, message))
    }
}

fn credential(value: &Option<String>, field: &str) -> Result<(), ConfigError> {
    if let Some(value) = value {
        let name = value.strip_prefix("env:").unwrap_or("");
        require(
            !name.is_empty()
                && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                && !name.as_bytes()[0].is_ascii_digit(),
            field,
            "expected an env:NAME credential reference",
        )?;
    }
    Ok(())
}

impl Config {
    pub(super) fn validate(&self) -> Result<EffectiveTimeouts, ConfigError> {
        for (count, field) in [
            (self.models.len(), "models"),
            (self.executors.len(), "executors"),
            (self.notifiers.len(), "notifiers"),
        ] {
            require(
                count <= super::MAX_PROFILES,
                field,
                "at most 64 profiles per category",
            )?;
        }
        require(
            self.schema_version == 1,
            "schema_version",
            "unsupported schema version",
        )?;
        require(
            self.paths.data_dir.is_absolute(),
            "paths.data_dir",
            "expected absolute data directory",
        )?;
        require(
            self.paths.socket_path.is_absolute(),
            "paths.socket_path",
            "expected absolute socket path",
        )?;
        require(
            self.paths.data_dir != self.paths.socket_path,
            "paths",
            "data directory and socket must differ",
        )?;
        for path in &self.access.read_roots {
            require(
                path.is_absolute(),
                "access.read_roots",
                "expected absolute read roots",
            )?;
        }
        require(
            self.consult.max_runner_starts > 0,
            "consult.max_runner_starts",
            "must be positive",
        )?;
        require(
            self.consult.max_delegation_depth < self.consult.max_runner_starts,
            "consult.max_delegation_depth",
            "depth must be smaller than the shared runner start budget",
        )?;
        require(
            self.consult.max_result_chars > 0,
            "consult.max_result_chars",
            "must be positive",
        )?;
        require(
            !self.consult.instructions_version.is_empty(),
            "consult.instructions_version",
            "must not be empty",
        )?;
        require(
            self.runtime.max_concurrent_jobs > 0,
            "runtime.max_concurrent_jobs",
            "must be positive; pause explicitly",
        )?;
        require(
            self.runtime.max_queued_jobs > 0,
            "runtime.max_queued_jobs",
            "must be positive",
        )?;
        require(
            (1..=2048).contains(&self.runtime.memory_budget_mib),
            "runtime.memory_budget_mib",
            "must be between 1 and 2048 MiB",
        )?;
        require(
            (1..=32).contains(&self.retrieval.top_k)
                && self.retrieval.top_k <= self.retrieval.max_candidates,
            "retrieval.top_k",
            "must be between 1 and 32 and within the candidate budget",
        )?;
        require(
            (1..=100).contains(&self.retrieval.max_candidates),
            "retrieval.max_candidates",
            "must be between 1 and 100",
        )?;
        require(
            self.retrieval.max_original_chars > 0,
            "retrieval.max_original_chars",
            "must be positive",
        )?;
        let t = &self.timeouts;
        let mut effective = EffectiveTimeouts {
            local_request: duration(&t.local_request, "timeouts.local_request")?,
            runner: duration(&t.runner, "timeouts.runner")?,
            workflow: Duration::ZERO,
            idle: if t.idle == "disabled" {
                None
            } else {
                Some(duration(&t.idle, "timeouts.idle")?)
            },
            queue: duration(&t.queue, "timeouts.queue")?,
            startup: duration(&t.startup, "timeouts.startup")?,
            cleanup: duration(&t.cleanup, "timeouts.cleanup")?,
            tools: duration(&t.tools, "timeouts.tools")?,
            summary: duration(&t.summary, "timeouts.summary")?,
            providers: BTreeMap::new(),
            executors: BTreeMap::new(),
        };
        for (name, model) in &self.models {
            require(
                !name.is_empty() && !model.model.is_empty(),
                "models.*.model",
                "model/profile names must not be empty",
            )?;
            require(
                matches!(model.endpoint.scheme(), "http" | "https")
                    && model.endpoint.host_str().is_some(),
                "models.*.endpoint",
                "expected an HTTP(S) endpoint",
            )?;
            require(
                model.endpoint.username().is_empty()
                    && model.endpoint.password().is_none()
                    && model.endpoint.query().is_none()
                    && model.endpoint.fragment().is_none(),
                "models.*.endpoint",
                "use credential_ref instead of URL credentials, queries or fragments",
            )?;
            credential(&model.credential_ref, "models.*.credential_ref")?;
            require(
                model.dimension.is_none_or(|d| (1..=65_536).contains(&d)),
                "models.*.dimension",
                "dimension must be between 1 and 65536",
            )?;
            require(
                model.kind != ModelKind::Embedding || model.dimension.is_some(),
                "models.*.dimension",
                "embedding requires its dimension",
            )?;
            effective.providers.insert(
                name.clone(),
                model
                    .timeout
                    .as_ref()
                    .map(|s| duration(s, "models.*.timeout"))
                    .transpose()?
                    .unwrap_or(effective.local_request),
            );
        }
        for (reference, kind, field) in [
            (
                &self.retrieval.embedding,
                ModelKind::Embedding,
                "retrieval.embedding",
            ),
            (
                &self.retrieval.reranker,
                ModelKind::Reranker,
                "retrieval.reranker",
            ),
            (
                &self.retrieval.extraction,
                ModelKind::Text,
                "retrieval.extraction",
            ),
        ] {
            if let Some(name) = reference {
                require(
                    self.models.get(name).is_some_and(|m| m.kind == kind),
                    field,
                    "missing model profile or wrong model kind",
                )?;
            }
        }
        let mut visiting = BTreeSet::new();
        let mut visited = BTreeSet::new();
        for (name, executor) in &self.executors {
            require(
                !name.is_empty() && name != "caller",
                "executors",
                "empty and caller profile names are reserved",
            )?;
            require(
                !executor.binary.as_os_str().is_empty(),
                "executors.*.binary",
                "binary must not be empty",
            )?;
            if let Some(model) = &executor.model {
                require(
                    self.models
                        .get(model)
                        .is_some_and(|m| m.kind == ModelKind::Text),
                    "executors.*.model",
                    "missing text model profile",
                )?;
            }
            self.check_fallback(name, &mut visiting, &mut visited)?;
            effective.executors.insert(
                name.clone(),
                executor
                    .timeout
                    .as_ref()
                    .map(|s| duration(s, "executors.*.timeout"))
                    .transpose()?
                    .unwrap_or(effective.runner),
            );
        }
        let selected: Vec<&str> = if self.consult.executor == "caller" {
            self.executors
                .iter()
                .filter(|(_, e)| e.enabled)
                .map(|(n, _)| n.as_str())
                .collect()
        } else {
            require(
                self.executors
                    .get(&self.consult.executor)
                    .is_some_and(|e| e.enabled),
                "consult.executor",
                "missing or disabled executor",
            )?;
            // Fallbacks are bounded by the same global start count.
            let mut selected = BTreeSet::new();
            self.collect_fallback(&self.consult.executor, &mut selected);
            selected.into_iter().collect()
        };
        let runner_max = selected
            .iter()
            .map(|name| effective.executors[*name])
            .max()
            .unwrap_or(effective.runner);
        let one = effective
            .startup
            .checked_add(runner_max)
            .and_then(|n| n.checked_add(effective.cleanup))
            .ok_or_else(|| ConfigError::invalid("timeouts.workflow", "deadline overflow"))?;
        let overhead = effective
            .tools
            .checked_add(effective.summary)
            .ok_or_else(|| ConfigError::invalid("timeouts.workflow", "deadline overflow"))?;
        let minimum = one
            .checked_add(overhead)
            .ok_or_else(|| ConfigError::invalid("timeouts.workflow", "deadline overflow"))?;
        effective.workflow = if t.workflow == "auto" {
            one.checked_mul(self.consult.max_runner_starts)
                .and_then(|n| n.checked_add(overhead))
                .ok_or_else(|| ConfigError::invalid("timeouts.workflow", "deadline overflow"))?
        } else {
            let requested = duration(&t.workflow, "timeouts.workflow")?;
            require(
                requested >= minimum,
                "timeouts.workflow",
                "shorter than one selected runner plus declared overhead",
            )?;
            requested
        };
        require(
            tokio::time::Instant::now()
                .checked_add(effective.workflow)
                .is_some(),
            "timeouts.workflow",
            "deadline exceeds monotonic clock range",
        )?;
        for (name, notifier) in &self.notifiers {
            require(
                !name.is_empty(),
                "notifiers",
                "channel name must not be empty",
            )?;
            require(
                notifier.kind != NotifierKind::Stdio
                    || notifier.command.first().is_some_and(|s| !s.is_empty()),
                "notifiers.*.command",
                "stdio notifier requires a command",
            )?;
            credential(&notifier.credential_ref, "notifiers.*.credential_ref")?;
        }
        Ok(effective)
    }

    fn check_fallback<'a>(
        &'a self,
        name: &'a str,
        visiting: &mut BTreeSet<&'a str>,
        visited: &mut BTreeSet<&'a str>,
    ) -> Result<(), ConfigError> {
        require(
            !visiting.contains(name),
            "executors.*.fallback",
            "recursive fallback",
        )?;
        if visited.contains(name) {
            return Ok(());
        }
        let executor = self.executors.get(name).ok_or_else(|| {
            ConfigError::invalid("executors.*.fallback", "missing fallback profile")
        })?;
        visiting.insert(name);
        for next in &executor.fallback {
            require(
                self.executors.get(next).is_some_and(|e| e.enabled),
                "executors.*.fallback",
                "missing or disabled fallback",
            )?;
            self.check_fallback(next, visiting, visited)?;
        }
        visiting.remove(name);
        visited.insert(name);
        Ok(())
    }

    fn collect_fallback<'a>(&'a self, name: &'a str, selected: &mut BTreeSet<&'a str>) {
        if selected.insert(name) {
            for next in &self.executors[name].fallback {
                self.collect_fallback(next, selected);
            }
        }
    }
}
