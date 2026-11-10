use super::*;
use crate::domain::{Generator, Ingress, Origin, Policy};
use crate::editor::{Classification, Feedback, PROMPT_VERSION, Report};

fn report(
    answer: &crate::adapter::ResearchConclusion,
    input: &Feedback,
    scope: &Scope,
) -> Result<Report> {
    let report = Report::parse(&answer.answer)?;
    if let Some(patch) = &report.patch {
        patch.validate()?;
        if &patch.scope != scope || patch.expires_at_ms != input.expires_at_ms {
            return Err(Error("SCOPE_DENIED"));
        }
        if patch.changes.iter().any(|edit| {
            !input.targets.iter().any(|target| {
                target.memory_id == edit.memory_id
                    && target.expected_revision == edit.expected_revision
            }) || !answer
                .memories
                .iter()
                .any(|m| m.memory_id == edit.memory_id && m.revision == edit.expected_revision)
        }) {
            return Err(Error("READ_DENIED"));
        }
        if patch
            .evidence
            .iter()
            .chain(patch.changes.iter().flat_map(|e| &e.draft.source_spans))
            .any(|source| !answer.sources.contains(source))
        {
            return Err(Error("INSUFFICIENT_EVIDENCE"));
        }
    }
    Ok(report)
}

impl Service {
    pub(in crate::daemon) async fn submit_editor(
        self: &Arc<Self>,
        request: String,
        feedback: Feedback,
    ) -> Result<Value> {
        feedback.validate()?;
        self.submit_editor_for(request, feedback, None).await
    }

    pub(in crate::daemon::jobs) async fn submit_incident_editor(
        self: &Arc<Self>,
        record: &crate::incident::Record,
    ) -> Result<Value> {
        let snapshot = self.retrieval_snapshot(record.config_generation)?;
        let feedback = Feedback {
            invocation_id: record.invocation_id.clone(),
            executor: snapshot
                .resolved()
                .config()
                .incidents
                .executor
                .clone()
                .ok_or(Error("EXECUTOR_UNAVAILABLE"))?,
            targets: record.signal.targets.clone(),
            feedback: record.signal.reason.clone(),
            expires_at_ms: record.expires_at_ms,
        };
        feedback.validate_targets(true)?;
        self.submit_editor_for(
            format!("incident/{}", record.id),
            feedback,
            Some(record.id.clone()),
        )
        .await
    }

    async fn submit_editor_for(
        self: &Arc<Self>,
        request: String,
        feedback: Feedback,
        incident: Option<String>,
    ) -> Result<Value> {
        if !ipc::identifier(&request) {
            return Err(Error("INVALID_REQUEST"));
        }
        let id = format!("editor/{}", hash(request.as_bytes()));
        let plan = {
            let _guard = self.admission.lock().await;
            self.require_local_job(&id)?;
            if self.storage.current(&job_key(&id))?.is_some() {
                let (_, job) = self.job(&id)?;
                self.authorize_job(&job)?;
                if job.consult_plan.as_ref().and_then(|p| p.editor.as_ref()) != Some(&feedback)
                    || job.consult_plan.as_ref().and_then(|p| p.incident.as_ref())
                        != incident.as_ref()
                {
                    return Err(Error("IDEMPOTENCY_CONFLICT"));
                }
                return Ok(serde_json::to_value(job)?);
            }
            if feedback.expires_at_ms <= ipc::now_ms()? {
                return Err(Error("PATCH_EXPIRED"));
            }
            let invocation =
                Invocation::load_verified(&self.storage, &feedback.invocation_id, &principal())?;
            invocation.ensure_local(&self.storage)?;
            let config = match &incident {
                Some(id) => {
                    let record = self.authorize_incident_execution(id)?;
                    self.retrieval_snapshot(record.config_generation)?
                }
                None => self.config.pin(),
            };
            let executor = config
                .resolved()
                .config()
                .executors
                .get(&feedback.executor)
                .ok_or(Error("EXECUTOR_UNAVAILABLE"))?;
            let native = NativeExecutor::from_profile(executor, None)?;
            self.authorize_executor(&feedback.executor, &native)?;
            let domain = Domain::new(&self.storage);
            let context = ReadContext::from_verified_binding(&principal(), invocation.id(), &id)?;
            for target in &feedback.targets {
                let memory = if incident.is_some() {
                    let memory = domain
                        .revision(&target.memory_id, target.expected_revision)?
                        .ok_or(Error("REVISION_NOT_FOUND"))?;
                    domain.permission(&context, Some(&memory.record.draft.scope))?;
                    memory
                } else {
                    domain.export_memory(&context, &target.memory_id).await?
                };
                if memory.revision.commit_seq != target.expected_revision {
                    return Err(Error("REVISION_CONFLICT"));
                }
                if !invocation
                    .diagnostic()
                    .scope
                    .contains(&memory.record.draft.scope)
                {
                    return Err(Error("SCOPE_DENIED"));
                }
            }
            Plan {
                invocation_id: invocation.id().into(),
                principal: principal(),
                invocation_revision: invocation.revision(),
                fence: invocation.fence_hash(),
                config_generation: config.generation(),
                executors: native.named(config.resolved().config(), &feedback.executor)?,
                native,
                args: ConsultArgs::new(None)?,
                editor: Some(feedback),
                incident,
            }
        };
        self.submit_job(id, JobInput::Consult(plan)).await
    }

    pub(in crate::daemon::jobs) fn authorize_editor(&self, plan: &Plan) -> Result<()> {
        if let Some(feedback) = &plan.editor {
            self.authorize_executor(&feedback.executor, &plan.native)?;
        }
        if let Some(id) = &plan.incident {
            let record = self.incident_record(id)?.1;
            if record.investigation != crate::incident::Investigation::Finished {
                self.authorize_incident_execution(id)?;
            }
        }
        Ok(())
    }

    pub(in crate::daemon::jobs) fn editor_checkpoint(&self, job: &Job) -> bool {
        job.consult_plan
            .as_ref()
            .is_some_and(|p| p.editor.is_some())
            && job
                .result
                .as_ref()
                .is_some_and(|r| r.get("editor_checkpoint").is_some())
    }

    pub(super) async fn editor_result(&self, id: &str, workflow: &Deadline) -> Result<Value> {
        let (initial_revision, original, plan, config, invocation) = {
            let _guard = self.admission.lock().await;
            let (revision, job) = self.job(id)?;
            self.authorize_job(&job)?;
            let plan = job.consult_plan.clone().ok_or(Error("JOB_CORRUPT"))?;
            let invocation = self.consult_invocation(&plan)?;
            let config = self.retrieval_snapshot(job.config_generation)?;
            if config.resolved().digest() != job.config_digest {
                return Err(Error("CONFIG_CORRUPT"));
            }
            (revision, job, plan, config, invocation)
        };
        let input = plan.editor.as_ref().ok_or(Error("JOB_CORRUPT"))?;
        let ready = if let Some(pointer) = original
            .result
            .as_ref()
            .and_then(|r| r.get("editor_checkpoint"))
        {
            models::load_result(&self.storage, pointer, initial_revision)?
        } else {
            let settings = &config.resolved().config().consult;
            let limits = if plan.incident.is_some() {
                let policy = &config.resolved().config().incidents;
                BudgetLimits {
                    starts: policy.max_runner_starts,
                    depth: settings
                        .max_delegation_depth
                        .min(policy.max_runner_starts - 1),
                    tokens: policy.max_tokens,
                    ..BudgetLimits::default()
                }
            } else {
                BudgetLimits {
                    starts: settings.max_runner_starts,
                    depth: settings.max_delegation_depth,
                    ..BudgetLimits::default()
                }
            };
            let budget = RunBudget::new(
                invocation.id(),
                &invocation.fence_hash(),
                self.runner_slots.clone(),
                limits,
                workflow.clone(),
            )?;
            let lease = {
                let _guard = self.admission.lock().await;
                let max = self
                    .config
                    .pin()
                    .resolved()
                    .config()
                    .runtime
                    .max_concurrent_jobs
                    .min(MAX_RUNNING);
                if MAX_RUNNING - self.runner_slots.available_permits() >= max {
                    return Err(Error("DELEGATION_UNAVAILABLE"));
                }
                budget.start()?
            };
            let mut research = research::Research::new(
                self,
                id,
                invocation.clone(),
                config.clone(),
                workflow,
                plan.clone(),
                budget.clone(),
            )?;
            let outcome = research.run(lease, plan.args.clone()).await;
            if outcome.is_err() {
                budget.cancel();
            }
            let answer = outcome?;
            report(&answer, input, &invocation.diagnostic().scope)?;
            let investigation_gaps = research.coverage_gaps(&answer)?;
            let trace = research.trace()?;
            let profile = trace["runs"]
                .as_array()
                .and_then(|runs| {
                    runs.iter()
                        .rev()
                        .find(|r| r["parent"].is_null() && r["state"] == "succeeded")
                })
                .and_then(|r| r["executor_profile"].as_str())
                .ok_or(Error("JOB_CORRUPT"))?;
            let model = config
                .resolved()
                .config()
                .executors
                .get(profile)
                .and_then(|e| e.model.as_ref())
                .and_then(|m| config.resolved().config().models.get(m))
                .map(|m| m.model.as_str())
                .unwrap_or("native-profile-default");
            let generator = Generator {
                runner: format!("executor/{profile}"),
                model: model.into(),
                prompt_version: PROMPT_VERSION.into(),
            };
            let ready = json!({"schema_version":1,"origin":"editor","answer":answer,"investigation_gaps":investigation_gaps,"trace":trace,"generator":generator,"executor":profile});
            let _guard = self.admission.lock().await;
            let (revision, mut job) = self.job(id)?;
            self.authorize_job(&job)?;
            if job.state != "running" || job.lease_owner.as_deref() != Some(&self.instance) {
                return Err(Error("LEASE_LOST"));
            }
            if job.cancel_requested {
                return Err(Error("CANCELLED"));
            }
            if workflow.expired().is_some() {
                return Err(Error("WORKFLOW_TIMEOUT"));
            }
            let pointer = models::save_result(&self.storage, &ready, config.generation()).await?;
            let mut trace = ready["trace"].clone();
            trace["editor_checkpoint"] = pointer;
            job.result = Some(trace);
            self.save_job(&job, Some(revision), vec![]).await?;
            ready
        };
        #[cfg(test)]
        crate::storage::crash_tests::checkpoint("editor-response");
        if ready["schema_version"] != 1 || ready["origin"] != "editor" {
            return Err(Error("JOB_CORRUPT"));
        }
        let answer: crate::adapter::ResearchConclusion =
            serde_json::from_value(ready["answer"].clone()).map_err(|_| Error("JOB_CORRUPT"))?;
        let gaps: Vec<String> = match ready.get("investigation_gaps") {
            Some(value) => {
                serde_json::from_value(value.clone()).map_err(|_| Error("JOB_CORRUPT"))?
            }
            // Older checkpoints cannot prove that the Runner reported every observed limitation.
            None => vec!["investigation_coverage_unavailable".into()],
        };
        if gaps.len() > 32 || gaps.iter().any(|gap| !ipc::identifier(gap)) {
            return Err(Error("JOB_CORRUPT"));
        }
        let report = report(&answer, input, &invocation.diagnostic().scope)?;
        let generator: Generator =
            serde_json::from_value(ready["generator"].clone()).map_err(|_| Error("JOB_CORRUPT"))?;
        let _guard = self.admission.lock().await;
        let (_, job) = self.job(id)?;
        self.authorize_job(&job)?;
        if job.cancel_requested {
            return Err(Error("CANCELLED"));
        }
        if workflow.expired().is_some() {
            return Err(Error("WORKFLOW_TIMEOUT"));
        }
        self.authorize_consult_result(&ready["trace"])?;
        let executor = ready["executor"].as_str().ok_or(Error("JOB_CORRUPT"))?;
        self.authorize_executor(
            executor,
            plan.executors.get(executor).ok_or(Error("JOB_CORRUPT"))?,
        )?;
        let context = self.job_read_context(&job)?;
        let domain = Domain::new(&self.storage);
        if let Some(id) = &plan.incident {
            self.authorize_incident_execution(id)?;
        }
        let mut result = ready["trace"].clone();
        result["editor"] = serde_json::to_value(&report)?;
        result["evidence"] = json!(answer.sources);
        result["conditions"] = json!(answer.conditions);
        result["conflicts"] = json!(answer.conflicts);
        result["gaps"] = json!(gaps);
        result["status"] = json!(answer.status);
        result["editor_checkpoint"] =
            job.result.as_ref().ok_or(Error("JOB_CORRUPT"))?["editor_checkpoint"].clone();
        if let Some(patch) = &report.patch {
            let ingress = Ingress::agent(
                "external-editor",
                id,
                patch.scope.clone(),
                Origin::MemoryEdit,
                generator,
            )?;
            let policy = Policy {
                revision: format!("{PROMPT_VERSION}/{}", config.resolved().digest()),
                config_generation: config.generation(),
                mode: if answer.status != RecallStatus::Found
                    || !gaps.is_empty()
                    || !answer.conflicts.is_empty()
                    || plan.incident.is_some()
                        && (!config.resolved().config().incidents.auto_repair
                            || !self.config.pin().resolved().config().incidents.auto_repair
                            || self.config.pin().resolved().config().review.mode
                                == crate::config::ReviewMode::Manual)
                {
                    crate::config::ReviewMode::Manual
                } else if plan.incident.is_some()
                    && self.config.pin().resolved().config().review.mode
                        == crate::config::ReviewMode::Policy
                    && config.resolved().config().review.mode
                        == crate::config::ReviewMode::Automatic
                {
                    crate::config::ReviewMode::Policy
                } else {
                    config.resolved().config().review.mode
                },
                scope: patch.scope.clone(),
                adopt_explicit_decisions: false,
            };
            result["patch_receipt"] = serde_json::to_value(
                domain
                    .apply_patch(&context, id, patch, &ingress, &policy, ipc::now_ms()?)
                    .await?,
            )?;
            #[cfg(test)]
            crate::storage::crash_tests::checkpoint("editor-applied");
        } else if matches!(
            report.classification,
            Classification::InsufficientEvidence
                | Classification::MemoryError
                | Classification::StaleConditions
        ) {
            let mut reviews = vec![];
            for (i, target) in input.targets.iter().enumerate() {
                reviews.push(
                    domain
                        .review_open(
                            &context,
                            &format!("{id}/review/{i}"),
                            &crate::domain::ReviewOpen {
                                memory_id: target.memory_id.clone(),
                                expected_revision: target.expected_revision,
                                reason: report.reason.clone(),
                            },
                            config.generation(),
                        )
                        .await?,
                );
            }
            result["reviews"] = json!(reviews);
        }
        self.wake_jobs();
        Ok(result)
    }
}
