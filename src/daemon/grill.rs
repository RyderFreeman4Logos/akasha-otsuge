use super::*;
use crate::adapter::{
    BudgetLimits, CancelReason, ConsultArgs, RunBudget, RunCollector, RunOptions, RunSpec,
};
use crate::config::{ResolvedConfig, deadline::Deadline};
use crate::domain::{Generator, Ingress, MemoryWrite, Origin, Policy, SourceDraft, SourceSpan};
use crate::grill::{Discussion, PROMPT_VERSION, Pending, Plan, Report, Role, Step};

impl Service {
    pub(in crate::daemon) async fn submit_grill(
        self: &Arc<Self>,
        id: String,
        plan: Plan,
    ) -> Result<Value> {
        self.submit_job(id, JobInput::Grill(plan)).await
    }
    pub(in crate::daemon) async fn resume_grill(
        self: &Arc<Self>,
        id: String,
        previous: String,
    ) -> Result<Value> {
        let mut plan = {
            let _guard = self.admission.lock().await;
            let (_, job) = self.job(&previous)?;
            self.authorize_job(&job)?;
            job.grill_plan.ok_or(Error("GRILL_NOT_RESUMABLE"))?
        };
        plan.resume_from = Some(previous);
        self.submit_grill(id, plan).await
    }
    pub(super) fn initial_grill(&self, plan: &Plan) -> Result<Discussion> {
        let Some(parent) = &plan.resume_from else {
            return Ok(Discussion::default());
        };
        let (revision, job) = self.job(parent)?;
        self.authorize_job(&job)?;
        let old = job
            .grill_plan
            .as_ref()
            .ok_or(Error("GRILL_NOT_RESUMABLE"))?;
        if !job.terminal()
            || old.scope != plan.scope
            || old.sources != plan.sources
            || old.question != plan.question
        {
            return Err(Error("GRILL_NOT_RESUMABLE"));
        }
        let pointer = job
            .result
            .as_ref()
            .and_then(|r| r.get("checkpoint"))
            .ok_or(Error("GRILL_NOT_RESUMABLE"))?;
        let saved = self.load_grill(pointer, revision)?;
        let baseline = [Role::Expert, Role::Reviewer]
            .into_iter()
            .filter_map(|role| saved.latest(role).cloned())
            .collect();
        Ok(Discussion {
            questions: saved.questions,
            baseline,
            ..Default::default()
        })
    }
    fn load_grill(&self, pointer: &Value, revision: u64) -> Result<Discussion> {
        let state: Discussion =
            serde_json::from_value(models::load_result(&self.storage, pointer, revision)?)
                .map_err(|_| Error("JOB_CORRUPT"))?;
        if state.schema_version != 1
            || state.questions.len() > 8
            || state.steps.len() > 17
            || state.baseline.len() > 2
            || (state.inflight.is_some() && state.pending.is_some())
        {
            return Err(Error("JOB_CORRUPT"));
        }
        for step in state.steps.iter().chain(&state.baseline) {
            for span in [&step.prompt, &step.output] {
                let source = Domain::new(&self.storage).source(&span.event_id, span.revision)?;
                Domain::new(&self.storage).permission(&context()?, Some(&source.payload.scope))?;
                if source.content_sha256 != span.content_sha256
                    || source.provenance.origin != Origin::AutoGrill
                    || source.payload.text.get(span.start..span.end).is_none()
                {
                    return Err(Error("JOB_CORRUPT"));
                }
            }
        }
        Ok(state)
    }
    pub(super) fn grill_checkpoint(&self, job: &Job, revision: u64) -> Result<Discussion> {
        match job.result.as_ref().and_then(|r| r.get("checkpoint")) {
            Some(pointer) => self.load_grill(pointer, revision),
            None => self.initial_grill(job.grill_plan.as_ref().ok_or(Error("JOB_CORRUPT"))?),
        }
    }
    async fn save_grill(&self, id: &str, state: &Discussion) -> Result<()> {
        let _guard = self.admission.lock().await;
        let (revision, mut job) = self.job(id)?;
        if job.state != "running" || job.lease_owner.as_deref() != Some(&self.instance) {
            return Err(Error("LEASE_LOST"));
        }
        let pointer = models::save_result(
            &self.storage,
            &serde_json::to_value(state)?,
            job.config_generation,
        )
        .await?;
        let discussion: Vec<_> = state.steps.iter().map(|step| json!({"stage":step.stage,"prompt":step.prompt,"output":step.output,"generator":step.generator})).collect();
        job.result = Some(
            json!({"origin":"auto_grill","prompt_version":PROMPT_VERSION,"checkpoint":pointer,"usage":state.usage,"stop_reason":state.stop_reason,"steps":state.steps.len(),"discussion":discussion,"attempt_prompt":state.attempt_prompt,"parent":job.grill_plan.as_ref().and_then(|p|p.resume_from.as_ref())}),
        );
        self.save_job(&job, Some(revision), vec![]).await
    }
    async fn grill_source(
        &self,
        job: &Job,
        index: usize,
        kind: &str,
        text: &str,
        generator: &Generator,
    ) -> Result<SourceSpan> {
        let plan = job.grill_plan.as_ref().ok_or(Error("JOB_CORRUPT"))?;
        let event_id = format!("grill/{}/{index}/{kind}", hash(job.id.as_bytes()));
        let ingress = Ingress::agent(
            "auto-grill",
            &job.id,
            plan.scope.clone(),
            Origin::AutoGrill,
            generator.clone(),
        )?;
        let receipt = Domain::new(&self.storage)
            .record_source(
                &event_id,
                &SourceDraft {
                    event_id: event_id.clone(),
                    scope: plan.scope.clone(),
                    text: text.into(),
                    reported_role: Some(kind.into()),
                    event_time_ms: None,
                    observed_time_ms: job.started_at_ms.ok_or(Error("JOB_CORRUPT"))? as i64,
                },
                &ingress,
                job.config_generation,
            )
            .await?;
        Ok(SourceSpan {
            event_id,
            revision: receipt.commit_seq,
            content_sha256: hash(text.as_bytes()),
            start: 0,
            end: text.len(),
        })
    }
    pub(super) async fn execute_grill(
        self: &Arc<Self>,
        id: String,
        workflow: Deadline,
    ) -> Result<()> {
        let (job, config, mut state) = {
            let _guard = self.admission.lock().await;
            let (revision, job) = self.job(&id)?;
            self.authorize_job(&job)?;
            let config = self.retrieval_snapshot(job.config_generation)?;
            if config.resolved().digest() != job.config_digest {
                return Err(Error("CONFIG_CORRUPT"));
            }
            let state = self.grill_checkpoint(&job, revision)?;
            (job, config, state)
        };
        let plan = job.grill_plan.as_ref().ok_or(Error("JOB_CORRUPT"))?;
        let settings = &config.resolved().config().grill;
        let budget = RunBudget::new(
            &job.id,
            &job.request_sha256,
            self.runner_slots.clone(),
            BudgetLimits {
                starts: settings.max_runner_starts,
                depth: 0,
                tokens: settings.max_tokens,
                ..Default::default()
            },
            workflow.clone(),
        )?
        .restore_usage(state.usage)?;
        let outcome: Result<()> = async {
            loop {
                {
                    let _guard = self.admission.lock().await;
                    let (_, current) = self.job(&id)?;
                    self.authorize_job(&current)?;
                    if current.cancel_requested {
                        return Err(Error("CANCELLED"));
                    }
                    if workflow.expired().is_some() {
                        return Err(Error("WORKFLOW_TIMEOUT"));
                    }
                    if self.stopping.load(std::sync::atomic::Ordering::Acquire) {
                        return Err(Error("RUNNER_INTERRUPTED"));
                    }
                }
                if let Some(pending) = state.pending.take() {
                    let records = plan.sources(&self.storage, &context()?)?;
                    let report = Report::parse(
                        &pending.text,
                        pending.stage,
                        &state.questions,
                        settings.max_questions,
                        plan,
                        &records,
                    )?;
                    let output = self
                        .grill_source(
                            &job,
                            state.steps.len(),
                            "response",
                            &pending.text,
                            &pending.generator,
                        )
                        .await?;
                    if pending.stage.role == Role::Interviewer {
                        state.questions = report.questions.clone();
                    }
                    state.steps.push(Step {
                        stage: pending.stage,
                        report,
                        prompt: pending.prompt,
                        output,
                        generator: pending.generator,
                    });
                    self.save_grill(&id, &state).await?;
                    #[cfg(test)]
                    crate::storage::crash_tests::checkpoint("grill-phase");
                }
                if let Some(reason) = state
                    .stop_reason
                    .clone()
                    .or_else(|| state.stopping(settings).map(String::from))
                {
                    state.stop_reason = Some(reason);
                    self.save_grill(&id, &state).await?;
                    break;
                }
                let stage = state.next();
                let profile = stage.role.profile(settings)?;
                let records = plan.sources(&self.storage, &context()?)?;
                let instructions = state.prompt(stage, plan, &records, settings.max_questions)?;
                let generator = Generator {
                    runner: format!("executor/{profile}"),
                    model: config.resolved().config().executors[profile]
                        .model
                        .as_ref()
                        .and_then(|name| config.resolved().config().models.get(name))
                        .map_or("native-profile-default", |m| m.model.as_str())
                        .into(),
                    prompt_version: PROMPT_VERSION.into(),
                };
                let lease = {
                    let _guard = self.admission.lock().await;
                    self.authorize_job(&job)?;
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
                let prompt = self
                    .grill_source(&job, state.steps.len(), "prompt", &instructions, &generator)
                    .await?;
                state.inflight = Some(stage);
                state.attempt_prompt = Some(prompt.clone());
                state.usage = budget.usage()?;
                self.save_grill(&id, &state).await?;
                let output = self
                    .grill_run(
                        &job,
                        config.resolved(),
                        &instructions,
                        profile,
                        lease,
                        &workflow,
                    )
                    .await;
                state.usage = budget.usage()?;
                match output {
                    Ok(text) => {
                        state.inflight = None;
                        state.pending = Some(Pending {
                            stage,
                            prompt,
                            text,
                            generator,
                        });
                        self.save_grill(&id, &state).await?;
                    }
                    Err(error) => {
                        state.inflight = None;
                        state.stop_reason = Some(
                            if error.0 == "BUDGET_EXHAUSTED" {
                                if state.usage.tokens >= settings.max_tokens {
                                    "token_budget"
                                } else {
                                    "resource_budget"
                                }
                            } else {
                                error.0
                            }
                            .into(),
                        );
                        self.save_grill(&id, &state).await?;
                        if error.0 == "BUDGET_EXHAUSTED" {
                            break;
                        }
                        return Err(error);
                    }
                }
                #[cfg(test)]
                crate::storage::crash_tests::checkpoint("grill-response");
            }
            Ok(())
        }
        .await;
        let _guard = self.admission.lock().await;
        let (revision, mut current) = self.job(&id)?;
        if current.state != "running" || current.lease_owner.as_deref() != Some(&self.instance) {
            return Err(Error("LEASE_LOST"));
        }
        if current.cancel_requested {
            return self
                .finish_job(current, revision, "cancelled", Some("CANCELLED"))
                .await;
        }
        if let Err(error) = outcome {
            return self
                .finish_job(current, revision, "failed", Some(error.0))
                .await;
        }
        let publication: Result<Vec<Value>> = async {
            self.authorize_job(&current)?;
            let records = plan.sources(&self.storage, &context()?)?;
            let drafts = state.drafts(plan, &records)?;
            let policy = Policy {
                revision: format!("{PROMPT_VERSION}/{}", job.config_generation),
                config_generation: job.config_generation,
                mode: config.resolved().config().review.mode,
                scope: plan.scope.clone(),
                adopt_explicit_decisions: false,
            };
            let domain = Domain::new(&self.storage);
            let mut candidates = vec![];
            for (index, (draft, generator)) in drafts.into_iter().enumerate() {
                if workflow.expired().is_some() {
                    return Err(Error("WORKFLOW_TIMEOUT"));
                }
                let memory_id = format!("grill/{}/{index}", hash(id.as_bytes()));
                let ingress = Ingress::agent(
                    "auto-grill",
                    &id,
                    plan.scope.clone(),
                    Origin::AutoGrill,
                    generator,
                )?;
                let receipt = domain
                    .propose(
                        &MemoryWrite {
                            request_id: memory_id.clone(),
                            memory_id: memory_id.clone(),
                            expected_revision: None,
                            draft,
                        },
                        &ingress,
                        &policy,
                    )
                    .await?;
                candidates.push(json!({"memory_id":memory_id,"revision":receipt.commit_seq}));
                #[cfg(test)]
                crate::storage::crash_tests::checkpoint("grill-candidate");
            }
            if workflow.expired().is_some() {
                return Err(Error("WORKFLOW_TIMEOUT"));
            }
            Ok(candidates)
        }
        .await;
        match publication {
            Ok(candidates) => {
                current.result.as_mut().ok_or(Error("JOB_CORRUPT"))?["candidates"] =
                    json!(candidates);
                current.result.as_mut().unwrap()["stop_reason"] = json!(state.stop_reason);
                self.finish_job(current, revision, "succeeded", None).await
            }
            Err(error) => {
                self.finish_job(current, revision, "failed", Some(error.0))
                    .await
            }
        }
    }

    async fn grill_run(
        &self,
        job: &Job,
        config: &ResolvedConfig,
        instructions: &str,
        profile: &str,
        lease: crate::adapter::RunLease,
        workflow: &Deadline,
    ) -> Result<String> {
        let native = job.grill_natives.get(profile).ok_or(Error("JOB_CORRUPT"))?;
        let directory = config.config().paths.data_dir.join("runner-work");
        private_dir(&directory)?;
        let (options, runner_timeout) = native.options(config, profile)?;
        let mut runner = native.runner(&directory, instructions).await?;
        let mut spec = RunSpec::for_grill(
            RunOptions {
                job_id: job.id.clone(),
                executor_profile: profile.into(),
                instructions_version: PROMPT_VERSION.into(),
                runner_timeout,
                args: ConsultArgs::new(None)?,
            },
            lease,
            job.config_generation,
        )?
        .with_guard(std::env::current_exe()?);
        spec.native_options = options;
        let lease = spec.lease_handle();
        let mut collector = RunCollector::new(&lease, crate::grill::MAX_REPORT_BYTES);
        let mut source = extraction::SuppliedEvidence;
        let mut handle = {
            let _guard = self.admission.lock().await;
            let (_, current) = self.job(&job.id)?;
            self.authorize_job(&current)?;
            if current.cancel_requested {
                return Err(Error("CANCELLED"));
            }
            if self.stopping.load(std::sync::atomic::Ordering::Acquire) {
                return Err(Error("RUNNER_INTERRUPTED"));
            }
            runner.start(spec, &mut source).await?
        };
        let run = async {
            while let Some(event) = handle.next().await? {
                collector.push(event)?;
            }
            collector.finish(handle.exit_status().await?)
        };
        let result = tokio::select! {
            biased;
            code=self.model_cancelled(&job.id) => Err(Error(if code=="PROVIDER_INTERRUPTED" {"RUNNER_INTERRUPTED"} else {code})),
            _=tokio::time::sleep(workflow.remaining()) => Err(Error("WORKFLOW_TIMEOUT")),
            result=run => result.map_err(Error::from),
        };
        if result.is_err() {
            handle.cancel(CancelReason::User).await?;
        }
        drop(handle);
        result
    }
}
