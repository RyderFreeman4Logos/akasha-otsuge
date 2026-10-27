//! Validated configuration generations; daemon persistence and IPC are separate consumers.

pub mod deadline;
mod manager;
mod schema;

pub use manager::{ConfigManager, ReloadReport, ReloadStatus, Snapshot};
pub use schema::*;

use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const MAX_CONFIG_BYTES: usize = 1024 * 1024;
const MAX_PROFILES: usize = 64;

#[derive(Clone, Debug, Serialize)]
pub struct ConfigError {
    pub code: &'static str,
    pub field: String,
    pub message: &'static str,
    pub byte_offset: Option<usize>,
}

impl ConfigError {
    fn invalid(field: &str, message: &'static str) -> Self {
        Self {
            code: "CONFIG_INVALID",
            field: field.into(),
            message,
            byte_offset: None,
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}: {}", self.code, self.field, self.message)
    }
}

impl std::error::Error for ConfigError {}

#[derive(Clone)]
pub struct ResolvedConfig {
    base_paths: Paths,
    selected_profile: Option<String>,
    config: Config,
    timeouts: EffectiveTimeouts,
    digest: String,
    sources: BTreeMap<String, String>,
    retrieval_fingerprint: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ArchivedConfig {
    base_paths: Paths,
    selected_profile: Option<String>,
    config: Config,
    sources: BTreeMap<String, String>,
}

impl ResolvedConfig {
    pub(crate) fn base_paths(&self) -> &Paths {
        &self.base_paths
    }
    pub(crate) fn archive(&self) -> ArchivedConfig {
        ArchivedConfig {
            base_paths: self.base_paths.clone(),
            selected_profile: self.selected_profile.clone(),
            config: self.config.clone(),
            sources: self.sources.clone(),
        }
    }
    pub(crate) fn restore(saved: ArchivedConfig) -> Result<Self, ConfigError> {
        let timeouts = saved.config.validate()?;
        Ok(Self {
            digest: digest(&saved.config),
            retrieval_fingerprint: digest(&(
                &saved.config.retrieval,
                &saved.config.models,
                &saved.config.access,
            )),
            config: saved.config,
            base_paths: saved.base_paths,
            selected_profile: saved.selected_profile,
            sources: saved.sources,
            timeouts,
        })
    }
    pub fn config(&self) -> &Config {
        &self.config
    }
    pub fn timeouts(&self) -> &EffectiveTimeouts {
        &self.timeouts
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn sources(&self) -> &BTreeMap<String, String> {
        &self.sources
    }
    pub fn retrieval_fingerprint(&self) -> &str {
        &self.retrieval_fingerprint
    }

    pub fn diagnostic(&self) -> Value {
        let mut config =
            serde_json::to_value(&self.config).expect("config has only JSON-compatible types");
        redact(&mut config);
        json!({
            "schema_version": self.config.schema_version,
            "selected_profile": self.selected_profile,
            "digest": self.digest,
            "config": config,
            "sources": self.sources,
            "effective_timeouts": self.timeouts,
            "retrieval_fingerprint": self.retrieval_fingerprint,
            "local_long_timeout_verified": false,
        })
    }
}

// Config values are not Debug: diagnostics always use the redacted projection.
impl fmt::Debug for ResolvedConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.diagnostic().fmt(f)
    }
}

fn redact(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for (name, value) in fields {
                if matches!(
                    name.as_str(),
                    "endpoint" | "credential_ref" | "args" | "command"
                ) && !value.is_null()
                {
                    *value = json!("[redacted]");
                } else {
                    redact(value);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                redact(value);
            }
        }
        _ => {}
    }
}

fn digest(value: &impl Serialize) -> String {
    let bytes = serde_json::to_vec(value).expect("config has only JSON-compatible types");
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn parse(input: &str) -> Result<toml::Value, ConfigError> {
    if input.len() > MAX_CONFIG_BYTES {
        return Err(ConfigError::invalid(
            "document",
            "configuration exceeds 1 MiB",
        ));
    }
    let parsed = toml::from_str::<toml::Value>(input).map_err(|e| ConfigError {
        byte_offset: e.span().map(|span| span.start),
        ..ConfigError::invalid("document", "invalid TOML; source values omitted")
    })?;
    if !parsed.is_table() {
        return Err(ConfigError::invalid("document", "expected a TOML table"));
    }
    Ok(parsed)
}

fn overlay(
    base: &mut toml::Value,
    layer: toml::Value,
    path: &str,
    source: &str,
    sources: &mut BTreeMap<String, String>,
) {
    match (base, layer) {
        (toml::Value::Table(base), toml::Value::Table(layer)) => {
            for (key, value) in layer {
                let path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                overlay(
                    base.entry(key)
                        .or_insert(toml::Value::Table(Default::default())),
                    value,
                    &path,
                    source,
                    sources,
                );
            }
        }
        (base, value) => {
            sources.insert(path.into(), source.into());
            *base = value;
        }
    }
}

fn typed(value: toml::Value) -> Result<Config, ConfigError> {
    serde_path_to_error::deserialize(value).map_err(|error| {
        // Dynamic profile names and unknown field names can themselves contain secrets.
        let path = error
            .path()
            .iter()
            .map(|segment| match segment {
                serde_path_to_error::Segment::Map { key }
                    if SAFE_FIELDS.contains(&key.as_str()) =>
                {
                    key.as_str()
                }
                _ => "*",
            })
            .collect::<Vec<_>>()
            .join(".");
        ConfigError::invalid(&path, "unknown field, invalid type or invalid value")
    })
}

const SAFE_FIELDS: &[&str] = &[
    "schema_version",
    "paths",
    "data_dir",
    "socket_path",
    "consult",
    "executor",
    "max_runner_starts",
    "max_delegation_depth",
    "max_result_chars",
    "instructions_version",
    "timeouts",
    "local_request",
    "runner",
    "workflow",
    "idle",
    "queue",
    "startup",
    "cleanup",
    "tools",
    "summary",
    "runtime",
    "max_concurrent_jobs",
    "max_queued_jobs",
    "memory_budget_mib",
    "paused",
    "retrieval",
    "top_k",
    "max_candidates",
    "max_original_chars",
    "embedding",
    "reranker",
    "extraction",
    "review",
    "mode",
    "access",
    "read_roots",
    "logging",
    "level",
    "executors",
    "models",
    "notifiers",
    "kind",
    "binary",
    "args",
    "enabled",
    "model",
    "timeout",
    "fallback",
    "endpoint",
    "local",
    "credential_ref",
    "dimension",
    "command",
];

fn default_sources(value: &Value, path: &str, sources: &mut BTreeMap<String, String>) {
    if let Value::Object(fields) = value {
        for (key, value) in fields {
            let path = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };
            default_sources(value, &path, sources);
        }
    } else {
        sources
            .entry(path.into())
            .or_insert_with(|| "builtin".into());
    }
}

/// Explicit paths avoid ambient working-directory or project-config authority.
/// Precedence: management overrides > selected profile > user file > defaults.
pub fn resolve(
    input: &str,
    profile: Option<&str>,
    overrides: &str,
    paths: &Paths,
) -> Result<ResolvedConfig, ConfigError> {
    let mut user = parse(input)?;
    let profiles = user
        .as_table_mut()
        .expect("parse checked table")
        .remove("profiles")
        .unwrap_or_else(|| toml::Value::Table(Default::default()));
    let profiles = profiles
        .as_table()
        .filter(|p| p.len() <= MAX_PROFILES)
        .ok_or_else(|| ConfigError::invalid("profiles", "expected at most 64 profiles"))?;
    let mut value = toml::Value::Table(Default::default());
    let mut sources = BTreeMap::new();
    overlay(
        &mut value,
        parse(include_str!("defaults.toml"))?,
        "",
        "builtin",
        &mut sources,
    );
    value.as_table_mut().expect("defaults table").insert(
        "paths".into(),
        toml::Value::try_from(paths)
            .map_err(|_| ConfigError::invalid("paths", "invalid path encoding"))?,
    );
    sources.insert("paths.data_dir".into(), "startup".into());
    sources.insert("paths.socket_path".into(), "startup".into());
    overlay(&mut value, user, "", "user", &mut sources);
    typed(value.clone())?.validate()?;
    for candidate in profiles.values() {
        if !candidate.is_table() {
            return Err(ConfigError::invalid("profiles", "profile must be a table"));
        }
        let mut trial = value.clone();
        overlay(
            &mut trial,
            candidate.clone(),
            "",
            "profile",
            &mut BTreeMap::new(),
        );
        typed(trial)?.validate()?;
    }
    if let Some(name) = profile {
        let selected = profiles
            .get(name)
            .ok_or_else(|| ConfigError::invalid("profiles", "selected profile does not exist"))?;
        overlay(&mut value, selected.clone(), "", "profile", &mut sources);
    }
    overlay(&mut value, parse(overrides)?, "", "override", &mut sources);
    let config = typed(value)?;
    let timeouts = config.validate()?;
    default_sources(
        &serde_json::to_value(&config).expect("validated JSON-compatible config"),
        "",
        &mut sources,
    );
    let retrieval_fingerprint = digest(&(&config.retrieval, &config.models, &config.access));
    Ok(ResolvedConfig {
        base_paths: paths.clone(),
        selected_profile: profile.map(str::to_owned),
        digest: digest(&config),
        config,
        timeouts,
        sources,
        retrieval_fingerprint,
    })
}

pub fn read_file(path: &Path) -> Result<String, ConfigError> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
        .open(path)
        .map_err(|_| ConfigError::invalid("document", "cannot open configuration file"))?;
    if !file.metadata().is_ok_and(|m| m.is_file()) {
        return Err(ConfigError::invalid(
            "document",
            "expected a regular configuration file",
        ));
    }
    let mut bytes = Vec::new();
    file.take((MAX_CONFIG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ConfigError::invalid("document", "cannot read configuration file"))?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(ConfigError::invalid(
            "document",
            "configuration exceeds 1 MiB",
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| ConfigError::invalid("document", "configuration must be UTF-8"))
}
