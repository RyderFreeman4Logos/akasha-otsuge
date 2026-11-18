use super::*;
use crate::config::deadline::Deadline;
use crate::retrieval::{Index, Request as RetrievalRequest};
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "retrieval/search.rs"]
mod search;

struct IndexBuild<'a>(&'a AtomicU64);
impl Drop for IndexBuild<'_> {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Release);
    }
}

impl Service {
    pub(in crate::daemon) async fn submit_retrieval(
        self: &Arc<Self>,
        id: String,
        plan: RetrievalRequest,
    ) -> Result<Value> {
        plan.validate()?;
        self.submit_job(id, JobInput::Retrieval(plan)).await
    }

    pub(in crate::daemon) fn retrieval_snapshot(
        &self,
        generation: u64,
    ) -> Result<Arc<crate::config::Snapshot<()>>> {
        let checkpoint = configuration::load(
            &self.storage,
            &format!("daemon/config-generation/{generation:020}"),
        )?
        .ok_or(Error("CONFIG_CORRUPT"))?;
        Ok(crate::config::ConfigManager::restore(checkpoint, |_| Ok(()))?.pin())
    }
    pub(super) fn query_vector_snapshot(
        &self,
        requested: &Arc<crate::config::Snapshot<()>>,
    ) -> Result<Arc<crate::config::Snapshot<()>>> {
        if let Some(vector) = Index::status(&self.storage, requested.resolved())?.vector
            && let Some(active) = crate::retrieval::active_vector(&self.storage)?
            && active.generation != vector.generation
            && active.config_generation <= requested.generation()
        {
            let previous = self.retrieval_snapshot(active.config_generation)?;
            let vector = Index::status(&self.storage, previous.resolved())?
                .vector
                .ok_or(Error("INDEX_CORRUPT"))?;
            if vector.generation != active.generation {
                return Err(Error("INDEX_CORRUPT"));
            }
            return Ok(previous);
        }
        Ok(requested.clone())
    }

    async fn index_cache(
        &self,
        id: &str,
        workflow: &Deadline,
    ) -> Result<tokio::sync::MutexGuard<'_, crate::retrieval::IndexCache>> {
        tokio::select! {
            guard=self.search_index.lock()=>Ok(guard),
            code=self.model_cancelled(id)=>Err(Error(code)),
            _=tokio::time::sleep(workflow.remaining())=>Err(Error("WORKFLOW_TIMEOUT")),
        }
    }

    async fn index_owner(
        &self,
        id: &str,
        workflow: &Deadline,
    ) -> Result<Arc<crate::retrieval::IndexOwner>> {
        let mut cache = self.index_cache(id, workflow).await?;
        Ok(cache.owner(self.config.pin().resolved()).await?)
    }

    pub(super) async fn retrieval_index(
        &self,
        id: &str,
        workflow: &Deadline,
    ) -> Result<Arc<Index>> {
        let mut cache = self.index_cache(id, workflow).await?;
        let owner = cache.owner(self.config.pin().resolved()).await?;
        if let Some(index) = &cache.published {
            return Ok(index.clone());
        }
        let index = Arc::new(
            Index::open_generation(owner, crate::retrieval::status(&self.storage)?, false).await?,
        );
        cache.published = Some(index.clone());
        Ok(index)
    }

    fn authorize_index_work(&self, id: &str, workflow: &Deadline) -> crate::storage::Result<()> {
        let (_, job) = self
            .job(id)
            .map_err(|e| crate::storage::StorageError::new(e.0))?;
        if job.state != "running" || job.lease_owner.as_deref() != Some(&self.instance) {
            return Err(crate::storage::StorageError::new("LEASE_LOST"));
        }
        self.authorize_job(&job)
            .map_err(|e| crate::storage::StorageError::new(e.0))?;
        let code = if job.cancel_requested {
            Some("CANCELLED")
        } else if self.stopping.load(Ordering::Acquire) {
            Some("INDEX_INTERRUPTED")
        } else if workflow.expired().is_some() {
            Some("WORKFLOW_TIMEOUT")
        } else {
            None
        };
        code.map_or(Ok(()), |code| Err(crate::storage::StorageError::new(code)))
    }

    async fn finish_rebuild(
        &self,
        id: &str,
        workflow: &Deadline,
        index: &Arc<Index>,
        progress: &crate::retrieval::Status,
        generation: u64,
        target: u64,
    ) -> Result<()> {
        let mut owned = self.index_cache(id, workflow).await?;
        let authorize = || self.authorize_index_work(id, workflow);
        // Existing reads own their Arc until their last native query completes.
        while owned.published.as_ref().is_some_and(|cached| {
            Arc::strong_count(cached) > 1 + usize::from(Arc::ptr_eq(index, cached))
        }) {
            authorize()?;
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        if index.is_pending() {
            Index::publish_rebuild(&self.storage, progress, generation, target, &authorize).await?;
            owned.published = None;
        }
        index
            .owner
            .finish_retirement(&self.storage, generation, &authorize)
            .await?;
        Ok(())
    }

    async fn retrieval_result(&self, id: &str, workflow: &Deadline) -> Result<Value> {
        let (plan, config, target, vector_generation) = {
            let _guard = self.admission.lock().await;
            let (_, job) = self.job(id)?;
            self.authorize_job(&job)?;
            let restored = self.retrieval_snapshot(job.config_generation)?;
            if restored.resolved().digest() != job.config_digest {
                return Err(Error("CONFIG_CORRUPT"));
            }
            (
                job.retrieval_plan.ok_or(Error("JOB_CORRUPT"))?,
                restored,
                job.retrieval_target_seq,
                job.retrieval_vector_config_generation,
            )
        };
        let _builder = if matches!(plan, RetrievalRequest::Sync | RetrievalRequest::Rebuild) {
            Some(tokio::select! {
                guard=self.index_builder.lock()=>guard,
                code=self.model_cancelled(id)=>return Err(Error(code)),
                _=tokio::time::sleep(workflow.remaining())=>return Err(Error("WORKFLOW_TIMEOUT")),
            })
        } else {
            None
        };
        let pending = match &plan {
            RetrievalRequest::Rebuild => {
                let mut owned = self
                    .search_index
                    .try_lock()
                    .map_err(|_| Error("RESOURCE_BUSY"))?;
                if owned
                    .published
                    .as_ref()
                    .is_some_and(|cached| Arc::strong_count(cached) > 1)
                {
                    return Err(Error("RESOURCE_BUSY"));
                }
                let authorize = || self.authorize_index_work(id, workflow);
                if let Some(pending) = crate::retrieval::pending_rebuild(&self.storage)?
                    && pending.generation == crate::retrieval::status(&self.storage)?.generation
                {
                    // Activation survived a stop. Release its old files before admitting another generation.
                    let owner = owned.owner(self.config.pin().resolved()).await?;
                    owner
                        .finish_retirement(&self.storage, config.generation(), &authorize)
                        .await?;
                }
                drop(owned);
                Some(
                    Index::start_rebuild(&self.storage, id, config.generation(), &authorize)
                        .await?,
                )
            }
            RetrievalRequest::Sync => crate::retrieval::pending_rebuild(&self.storage)?,
            _ => None,
        };
        let needs_retirement = pending.is_some();
        let index = match pending {
            Some(progress)
                if progress.generation != crate::retrieval::status(&self.storage)?.generation =>
            {
                let owner = self.index_owner(id, workflow).await?;
                Arc::new(
                    Index::open_candidate(
                        &self.storage,
                        &owner,
                        &config,
                        progress,
                        id,
                        matches!(plan, RetrievalRequest::Rebuild),
                        &|| self.authorize_index_work(id, workflow),
                    )
                    .await?,
                )
            }
            _ => self.retrieval_index(id, workflow).await?,
        };
        if workflow.expired().is_some() {
            return Err(Error("WORKFLOW_TIMEOUT"));
        }
        match plan {
            RetrievalRequest::Sync | RetrievalRequest::Rebuild => {
                let target = target.ok_or(Error("JOB_CORRUPT"))?;
                let mut candidate = index;
                let mut retry = matches!(plan, RetrievalRequest::Rebuild);
                loop {
                    let result = self
                        .maintain_index(id, workflow, &candidate, &config, target, needs_retirement)
                        .await;
                    if !candidate.is_pending()
                        || !matches!(
                            &result,
                            Err(Error("INDEX_IO" | "INDEX_CORRUPT" | "INDEX_MISSING"))
                        )
                    {
                        return result;
                    }
                    let authorize = || self.authorize_index_work(id, workflow);
                    if !candidate
                        .mark_damaged_candidate(&self.storage, config.generation(), &authorize)
                        .await?
                        || !retry
                    {
                        return result;
                    }
                    retry = false;
                    let damaged = crate::retrieval::pending_rebuild(&self.storage)?
                        .ok_or(Error("INDEX_NOT_READY"))?;
                    let owner = self.index_owner(id, workflow).await?;
                    drop(candidate);
                    candidate = Arc::new(
                        Index::open_candidate(
                            &self.storage,
                            &owner,
                            &config,
                            damaged,
                            id,
                            false,
                            &authorize,
                        )
                        .await?,
                    );
                }
            }
            plan @ RetrievalRequest::Search { .. } => {
                self.search(id, workflow, index, config, vector_generation, plan)
                    .await
            }
        }
    }

    async fn maintain_index(
        &self,
        id: &str,
        workflow: &Deadline,
        index: &Arc<Index>,
        config: &crate::config::Snapshot<()>,
        target: u64,
        needs_retirement: bool,
    ) -> Result<Value> {
        let context = context()?;
        let scope = owner_scope();
        let authorize = || {
            Domain::new(&self.storage)
                .permission(&context, Some(&scope))
                .map(|_| ())
        };
        self.index_epoch.fetch_add(1, Ordering::Release);
        let _epoch = IndexBuild(&self.index_epoch);
        let mut windows_since_maintenance = 0u8;
        loop {
            {
                let _guard = self.admission.lock().await;
                let (_, job) = self.job(id)?;
                self.authorize_job(&job)?;
                if job.cancel_requested {
                    return Err(Error("CANCELLED"));
                }
                if self.stopping.load(std::sync::atomic::Ordering::Acquire) {
                    return Err(Error("INDEX_INTERRUPTED"));
                }
                if workflow.expired().is_some() {
                    return Err(Error("WORKFLOW_TIMEOUT"));
                }
            }
            let mut progress = index
                .sync_batch(&self.storage, target, config.generation(), &authorize)
                .await?;
            // Keep the writer's pinned vector even if a search opens another generation.
            let vector = if config.resolved().config().retrieval.embedding.is_some() {
                Some(index.vector_index(&self.storage, config.resolved()).await?)
            } else {
                None
            };
            if let Some(vector) = &vector {
                progress.vector = Some(
                    match vector
                        .sync_batch(
                            &self.storage,
                            target,
                            config,
                            &self.providers,
                            workflow,
                            || self.model_cancelled(id),
                            &authorize,
                        )
                        .await
                    {
                        Ok(progress) => progress,
                        Err(err) if err.code == "INDEX_INPUT_CHANGED" => continue,
                        Err(err) => return Err(err.into()),
                    },
                );
            }
            let complete = progress.lexical_watermark >= target
                && progress
                    .vector
                    .as_ref()
                    .is_none_or(|v| v.watermark >= target);
            windows_since_maintenance = windows_since_maintenance.saturating_add(1);
            if complete || windows_since_maintenance >= 32 {
                let authorize_native = || {
                    authorize()?;
                    self.authorize_index_work(id, workflow)
                };
                index
                    .maintain_tables(vector.as_deref(), &authorize_native)
                    .await?;
                windows_since_maintenance = 0;
                if index.is_pending() {
                    index.prune_versions(&authorize_native).await?;
                } else if let Ok(owned) = self.search_index.try_lock()
                    && owned.published.as_ref().is_some_and(|cached| {
                        Arc::ptr_eq(cached, index) && Arc::strong_count(cached) == 2
                    })
                {
                    // New readers wait for native pruning behind this cache lock.
                    index.prune_versions(&authorize_native).await?;
                }
            }
            if complete {
                if index.is_pending() {
                    index
                        .check_candidate_files(&|| self.authorize_index_work(id, workflow))
                        .await?;
                }
                crate::retrieval::activate_vector(
                    &self.storage,
                    &progress,
                    config,
                    target,
                    &|| {
                        authorize()?;
                        self.authorize_index_work(id, workflow)
                    },
                )
                .await?;
                if needs_retirement {
                    self.finish_rebuild(
                        id,
                        workflow,
                        index,
                        &progress,
                        config.generation(),
                        target,
                    )
                    .await?;
                    progress = Index::status(&self.storage, config.resolved())?;
                }
                return Ok(serde_json::to_value(progress)?);
            }
        }
    }

    pub(super) async fn execute_retrieval(
        self: &Arc<Self>,
        id: String,
        workflow: Deadline,
    ) -> Result<()> {
        let outcome = self.retrieval_result(&id, &workflow).await;
        let _guard = self.admission.lock().await;
        let (revision, mut job) = self.job(&id)?;
        if job.state != "running" || job.lease_owner.as_deref() != Some(&self.instance) {
            return Err(Error("LEASE_LOST"));
        }
        if job.cancel_requested {
            return self
                .finish_job(job, revision, "cancelled", Some("CANCELLED"))
                .await;
        }
        if let Err(error) = self.authorize_job(&job) {
            return self
                .finish_job(job, revision, "failed", Some(error.0))
                .await;
        }
        match outcome {
            Ok(value) => {
                job.result = Some(value);
                if workflow.expired().is_some() {
                    self.finish_job(job, revision, "failed", Some("WORKFLOW_TIMEOUT"))
                        .await
                } else {
                    self.finish_job(job, revision, "succeeded", None).await
                }
            }
            Err(error) => {
                self.finish_job(job, revision, "failed", Some(error.0))
                    .await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::fixture;
    use super::*;

    async fn versions(table: &lancedb::Table) -> Vec<u64> {
        let mut versions = table
            .list_versions()
            .await
            .unwrap()
            .into_iter()
            .map(|v| v.version)
            .collect::<Vec<_>>();
        versions.sort_unstable();
        versions
    }

    #[tokio::test]
    async fn native_pruning_requires_running_job_and_current_lease_owner() {
        let mut observations = Vec::new();
        for (state, own_lease) in [("queued", true), ("running", false)] {
            let (fixture, service, _) = fixture(&format!("prune-lease-{state}"), "").await;
            let id = "lease-index";
            service
                .submit_retrieval(id.into(), RetrievalRequest::Sync)
                .await
                .unwrap();
            assert!(service.claim(id, 0).await.unwrap());
            let workflow = Deadline::start(
                crate::config::deadline::Layer::Workflow,
                Duration::from_secs(30),
                None,
            )
            .unwrap();
            let index = service.retrieval_index(id, &workflow).await.unwrap();
            let target = service.storage.sequence().unwrap();
            let progress = index
                .sync_batch(
                    &service.storage,
                    target,
                    service.config.pin().generation(),
                    &|| Ok(()),
                )
                .await
                .unwrap();
            assert!(progress.lexical_watermark >= target);
            drop(index);
            let connection = lancedb::connect(fixture.0.join("data/search").to_str().unwrap())
                .execute()
                .await
                .unwrap();
            let table = connection
                .open_table("memory_lexical_v1")
                .execute()
                .await
                .unwrap();
            let before = versions(&table).await;
            assert!(before.len() > 1, "fixture needs obsolete native versions");
            let (revision, mut job) = service.job(id).unwrap();
            job.state = state.into();
            job.lease_owner = Some(if own_lease {
                service.instance.clone()
            } else {
                "another-daemon".into()
            });
            service
                .save_job(&job, Some(revision), vec![])
                .await
                .unwrap();
            let denied = service
                .retrieval_result(id, &workflow)
                .await
                .map(|_| ())
                .map_err(|e| e.0);
            let after_denial = versions(&table).await;
            let (revision, mut job) = service.job(id).unwrap();
            job.state = "running".into();
            job.lease_owner = Some(service.instance.clone());
            service
                .save_job(&job, Some(revision), vec![])
                .await
                .unwrap();
            let allowed = service
                .retrieval_result(id, &workflow)
                .await
                .map(|_| ())
                .map_err(|e| e.0);
            let after_allowed = versions(&table).await;
            drop(table);
            drop(connection);
            service.close(Ok(())).await.unwrap();
            observations.push((state, denied, before, after_denial, allowed, after_allowed));
        }
        for (state, denied, before, after_denial, allowed, after_allowed) in observations {
            assert_eq!(
                denied,
                Err("LEASE_LOST"),
                "{state}: {before:?} -> {after_denial:?}"
            );
            assert_eq!(
                after_denial, before,
                "{state}: denied maintenance removed native versions"
            );
            assert_eq!(allowed, Ok(()));
            assert_eq!(after_allowed, vec![*before.last().unwrap()]);
        }
    }
}
