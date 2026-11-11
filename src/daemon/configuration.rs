use super::*;
use crate::config::{self, ConfigManager};
use crate::storage::{Change, CommitRequest, RecordKey, RecordKind, RevisionRef, hash};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_ARCHIVE: usize = 8 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pointer {
    schema_version: u32,
    bytes: usize,
    sha256: String,
    parts: Vec<RevisionRef>,
}
fn key(id: &str) -> RecordKey {
    RecordKey {
        kind: RecordKind::State,
        id: id.into(),
    }
}

pub(super) fn load(storage: &Storage, id: &str) -> Result<Option<Value>> {
    let Some(revision) = storage.current(&key(id))? else {
        return Ok(None);
    };
    let pointer: Pointer = serde_json::from_str(&revision.content)?;
    if pointer.schema_version != 1
        || pointer.bytes > MAX_ARCHIVE
        || pointer.parts.len() > MAX_ARCHIVE / 16384 + 1
    {
        return Err(Error("CONFIG_CORRUPT"));
    }
    let mut content = String::new();
    for part in pointer.parts {
        if part.key.kind != RecordKind::Evidence
            || !part.key.id.starts_with("daemon-config-blob/")
            || part.commit_seq > revision.commit_seq
        {
            return Err(Error("CONFIG_CORRUPT"));
        }
        let chunk = storage
            .revision(&part.key, part.commit_seq)?
            .ok_or(Error("CONFIG_CORRUPT"))?;
        if chunk.previous.is_some() || content.len() + chunk.content.len() > pointer.bytes {
            return Err(Error("CONFIG_CORRUPT"));
        }
        content.push_str(&chunk.content);
    }
    if content.len() != pointer.bytes || hash(content.as_bytes()) != pointer.sha256 {
        return Err(Error("CONFIG_CORRUPT"));
    }
    Ok(Some(serde_json::from_str(&content)?))
}

pub(super) async fn save(storage: &Storage, value: &Value) -> Result<()> {
    let generation = value["generation"]
        .as_u64()
        .ok_or(Error("CONFIG_CORRUPT"))?;
    let content = serde_json::to_string(value)?;
    if content.len() > MAX_ARCHIVE {
        return Err(Error("CONFIG_TOO_LARGE"));
    }
    let mut parts = Vec::new();
    let mut start = 0;
    while start < content.len() {
        let mut end = (start + 16384).min(content.len());
        while !content.is_char_boundary(end) {
            end -= 1;
        }
        let chunk = &content[start..end];
        let digest = hash(chunk.as_bytes());
        let key = RecordKey {
            kind: RecordKind::Evidence,
            id: format!("daemon-config-blob/{digest}"),
        };
        let seq = if let Some(existing) = storage.current(&key)? {
            if existing.content != chunk || existing.previous.is_some() {
                return Err(Error("CONFIG_CORRUPT"));
            }
            existing.commit_seq
        } else {
            storage
                .commit(&CommitRequest {
                    request_id: format!("config-blob/{digest}"),
                    config_generation: generation,
                    policy_revision: None,
                    changes: vec![Change {
                        key: key.clone(),
                        expected_revision: None,
                        content: chunk.into(),
                    }],
                })
                .await?
                .commit_seq
        };
        parts.push(RevisionRef {
            key,
            commit_seq: seq,
        });
        start = end;
    }
    let digest = hash(content.as_bytes());
    let pointer = serde_json::to_string(&Pointer {
        schema_version: 1,
        bytes: content.len(),
        sha256: digest.clone(),
        parts,
    })?;
    let root = key("daemon/config");
    let previous = storage.current(&root)?.map(|r| r.commit_seq);
    let mut changes = vec![Change {
        key: root,
        expected_revision: previous,
        content: pointer.clone(),
    }];
    let generation_key = key(&format!("daemon/config-generation/{generation:020}"));
    if storage.current(&generation_key)?.is_none() {
        changes.push(Change {
            key: generation_key,
            expected_revision: None,
            content: pointer,
        });
    }
    storage
        .commit(&CommitRequest {
            request_id: format!("config/{}/{digest}", previous.unwrap_or(0)),
            config_generation: generation,
            policy_revision: None,
            changes,
        })
        .await?;
    Ok(())
}

impl Service {
    pub(super) async fn reload(self: &Arc<Self>) -> Result<Value> {
        let service = self.clone();
        // Ownership outlives a disconnected CLI or request timeout.
        self.operation(async move {
            let _guard = service.admission.lock().await;
            let input = config::read_file(&service.config_path)?;
            let digest = hash(input.as_bytes());
            if let Some((previous, report)) = service.last_config.lock().await.as_ref()
                && previous == &digest
            {
                return Ok(report.clone());
            }
            let runtime = tokio::runtime::Handle::current();
            let worker = service.clone();
            let report = tokio::task::spawn_blocking(move || {
                worker.config.reload_committing(
                    &input,
                    worker.profile.as_deref(),
                    |_| Ok(()),
                    |value| {
                        runtime
                            .block_on(save(&worker.storage, value))
                            .map_err(|_| ())
                    },
                )
            })
            .await
            .map_err(|_| Error("CONFIG_INVALID"))??;
            let report = serde_json::to_value(report)?;
            *service.last_config.lock().await = Some((digest, report.clone()));
            service.wake_jobs();
            Ok(report)
        })
        .await
    }
    pub(super) async fn config_status(&self) -> Result<Value> {
        let _guard = self.admission.lock().await;
        Ok(self.config.status()?)
    }
    pub(super) fn validate(&self) -> Result<Value> {
        let input = config::read_file(&self.config_path)?;
        let active = self.config.pin();
        Ok(config::resolve(
            &input,
            self.profile.as_deref(),
            "",
            active.resolved().base_paths(),
        )?
        .diagnostic())
    }
}

pub(super) async fn open(storage: &Storage, initial: ResolvedConfig) -> Result<ConfigManager<()>> {
    let imported_root = storage.import_identity()?.is_some()
        && !storage.locally_written(
            storage
                .current(&key("daemon/config"))?
                .map_or(0, |r| r.commit_seq),
        )?;
    let manager = if imported_root {
        let mut generation = 1_u64;
        let mut after = None;
        loop {
            let keys = storage.head_keys(
                &key("daemon/config-generation/00000000000000000000"),
                &key("daemon/config-generation/zzzzzzzzzzzzzzzzzzzz"),
                after.as_deref(),
                32,
            )?;
            for key in keys.iter().take(32) {
                let value: u64 = key
                    .id
                    .rsplit('/')
                    .next()
                    .ok_or(Error("CONFIG_CORRUPT"))?
                    .parse()
                    .map_err(|_| Error("CONFIG_CORRUPT"))?;
                generation = generation.max(value.checked_add(1).ok_or(Error("CONFIG_CORRUPT"))?);
                after = Some(key.id.clone());
            }
            if keys.len() <= 32 {
                break;
            }
        }
        let manager = ConfigManager::new(generation, initial, |_| Ok(()))?;
        save(storage, &manager.checkpoint()?).await?;
        #[cfg(test)]
        crate::storage::crash_tests::checkpoint("import-runtime-config");
        manager
    } else if let Some(checkpoint) = load(storage, "daemon/config")? {
        let manager = ConfigManager::restore(checkpoint, |_| Ok(()))?;
        let generation = manager.pin().generation();
        let manager = manager.restarted(initial, |_| Ok(()))?;
        if manager.pin().generation() != generation {
            save(storage, &manager.checkpoint()?).await?;
        }
        manager
    } else {
        let manager = ConfigManager::new(1, initial, |_| Ok(()))?;
        save(storage, &manager.checkpoint()?).await?;
        manager
    };
    initialize_imported_controls(storage, manager.pin().generation()).await?;
    Ok(manager)
}

async fn initialize_imported_controls(storage: &Storage, generation: u64) -> Result<()> {
    let Some(identity) = storage.import_identity()? else {
        return Ok(());
    };
    let mut changes = Vec::new();
    for (id, content) in [
        ("daemon/active-jobs", "[]".into()),
        ("daemon/incidents", "[]".into()),
        ("notification/cursor", identity.commit_seq.to_string()),
    ] {
        let key = key(id);
        let current = storage.current(&key)?;
        if !storage.locally_written(current.as_ref().map_or(0, |r| r.commit_seq))? {
            changes.push(Change {
                key,
                expected_revision: current.map(|r| r.commit_seq),
                content,
            });
        }
    }
    if !changes.is_empty() {
        storage
            .commit(&CommitRequest {
                request_id: format!(
                    "import-runtime/{}/{}",
                    identity.archive_sha256,
                    hash(&serde_json::to_vec(&changes)?)
                ),
                config_generation: generation,
                policy_revision: None,
                changes,
            })
            .await?;
        #[cfg(test)]
        crate::storage::crash_tests::checkpoint("import-runtime-controls");
    }
    if !storage.locally_written(
        storage
            .current(&key("search/lexical-v1"))?
            .map_or(0, |r| r.commit_seq),
    )? {
        crate::retrieval::Index::begin_rebuild(
            storage,
            &format!("import/{}", identity.archive_sha256),
            generation,
            &|| Ok(()),
        )
        .await?;
        #[cfg(test)]
        crate::storage::crash_tests::checkpoint("import-runtime-index");
    }
    Ok(())
}
