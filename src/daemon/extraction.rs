use super::*;
use crate::adapter::{
    BudgetLimits, CancelReason, ConsultArgs, ContextRequest, ContextResponse, ContextSource,
    RunBudget, RunCollector, RunLease, RunOptions, RunSpec,
};
use crate::config::{ResolvedConfig, deadline::Deadline};
use crate::domain::Generator;
use crate::extraction::{self, PROMPT_VERSION, Plan};

pub(super) struct SuppliedEvidence;
impl ContextSource for SuppliedEvidence {
    fn execute<'a>(
        &'a mut self,
        lease: &'a RunLease,
        _request: ContextRequest,
    ) -> futures::future::BoxFuture<'a, crate::adapter::Result<ContextResponse>> {
        Box::pin(async move {
            lease.check()?;
            Err(crate::adapter::ErrorCode::ToolDenied.into())
        })
    }
}

impl Service {
    pub(in crate::daemon) async fn submit_extraction(
        self: &Arc<Self>,
        id: String,
        plan: Plan,
    ) -> Result<Value> {
        self.submit_job(id, JobInput::Extraction(plan)).await
    }

    pub(super) fn extraction_call(
        &self,
        plan: &Plan,
        config: &ResolvedConfig,
    ) -> Result<crate::provider::Call> {
        let profile = config
            .config()
            .retrieval
            .extraction
            .clone()
            .ok_or(Error("MODEL_NOT_CONFIGURED"))?;
        Ok(crate::provider::Call {
            profile,
            input: crate::provider::Input::Text {
                prompt: plan.prompt(&self.storage, &context()?)?,
                max_output_tokens: 4096,
            },
        })
    }

    pub(super) async fn execute_extraction(
        self: &Arc<Self>,
        id: String,
        workflow: Deadline,
    ) -> Result<()> {
        let (original_revision, original_job, config) = {
            let _guard = self.admission.lock().await;
            let (revision, job) = self.job(&id)?;
            self.authorize_job(&job)?;
            let config = self.retrieval_snapshot(job.config_generation)?;
            if config.resolved().digest() != job.config_digest {
                return Err(Error("CONFIG_CORRUPT"));
            }
            (revision, job, config)
        };
        let plan = original_job
            .extraction_plan
            .as_ref()
            .ok_or(Error("JOB_CORRUPT"))?;
        let profile = config
            .resolved()
            .config()
            .retrieval
            .extraction
            .as_deref()
            .ok_or(Error("MODEL_NOT_CONFIGURED"))?;
        let mut trace = json!({"deadline":workflow.trace(),"origin":"extraction"});
        let checkpoint = original_job
            .result
            .as_ref()
            .and_then(|v| v.get("checkpoint"))
            .cloned();
        let (text, error, generator) = if let Some(pointer) = &checkpoint {
            let saved = models::load_result(&self.storage, pointer, original_revision)?;
            if saved["schema_version"] != 1
                || saved["prompt_version"] != PROMPT_VERSION
                || saved["origin"] != "extraction"
            {
                return Err(Error("JOB_CORRUPT"));
            }
            let text = saved["text"]
                .as_str()
                .ok_or(Error("JOB_CORRUPT"))?
                .to_owned();
            let generator = serde_json::from_value::<Generator>(saved["generator"].clone())
                .map_err(|_| Error("JOB_CORRUPT"))?;
            trace = saved["trace"].clone();
            (Some(text), None, generator)
        } else if let Some(name) = profile.strip_prefix("executor:") {
            let result = self
                .native_extraction(&original_job, config.resolved(), &workflow, &mut trace)
                .await;
            let model = config
                .resolved()
                .config()
                .executors
                .get(name)
                .and_then(|e| e.model.as_ref())
                .and_then(|m| config.resolved().config().models.get(m))
                .map(|m| m.model.as_str())
                .unwrap_or("native-profile-default");
            let generator = Generator {
                runner: format!("executor/{name}"),
                model: model.into(),
                prompt_version: PROMPT_VERSION.into(),
            };
            match result {
                Ok(text) => (Some(text), None, generator),
                Err(error) => (None, Some(error.0.to_string()), generator),
            }
        } else {
            if original_job.extraction_native.is_some() {
                return Err(Error("JOB_CORRUPT"));
            }
            let call = self.extraction_call(plan, config.resolved())?;
            let outcome = self
                .providers
                .call_guarded(
                    config.resolved(),
                    &call,
                    &workflow,
                    self.model_cancelled(&id),
                    self.authorize_model(&id),
                )
                .await;
            trace = json!(outcome.trace);
            let generator = Generator {
                runner: format!("text/{profile}"),
                model: config.resolved().config().models[profile].model.clone(),
                prompt_version: PROMPT_VERSION.into(),
            };
            (
                outcome
                    .output
                    .and_then(|v| v["text"].as_str().map(String::from)),
                outcome.error,
                generator,
            )
        };
        let _guard = self.admission.lock().await;
        let (mut revision, mut job) = self.job(&id)?;
        if job.state != "running" || job.lease_owner.as_deref() != Some(&self.instance) {
            return Err(Error("LEASE_LOST"));
        }
        job.result = Some(
            json!({"origin":"extraction","prompt_version":PROMPT_VERSION,"trace":trace,"generator":generator}),
        );
        if job.cancel_requested {
            return self
                .finish_job(job, revision, "cancelled", Some("CANCELLED"))
                .await;
        }
        if let Some(reason) = error {
            return self
                .finish_job(job, revision, "failed", Some(&reason))
                .await;
        }
        if workflow.expired().is_some() {
            return self
                .finish_job(job, revision, "failed", Some("WORKFLOW_TIMEOUT"))
                .await;
        }
        let prepared = (|| {
            self.authorize_job(&job)?;
            let records = plan.sources(&self.storage, &context()?)?;
            let text = text.as_deref().ok_or(Error("EXTRACTION_OUTPUT_INVALID"))?;
            Ok::<_, Error>(extraction::parse(plan, text, &records)?)
        })();
        let drafts = match prepared {
            Ok(drafts) => drafts,
            Err(error) => {
                return self
                    .finish_job(job, revision, "failed", Some(error.0))
                    .await;
            }
        };
        if let Some(pointer) = checkpoint {
            job.result.as_mut().unwrap()["checkpoint"] = pointer;
        } else {
            let saved = json!({"schema_version":1,"origin":"extraction","prompt_version":PROMPT_VERSION,"text":text,"generator":generator,"trace":trace});
            let pointer = models::save_result(&self.storage, &saved, job.config_generation).await?;
            job.result.as_mut().unwrap()["checkpoint"] = pointer;
            self.save_job(&job, Some(revision), vec![]).await?;
            revision = self.job(&id)?.0;
        }
        #[cfg(test)]
        crate::storage::crash_tests::checkpoint("extraction-response");
        let candidates = extraction::publish(
            &self.storage,
            plan,
            &id,
            job.config_generation,
            generator.clone(),
            drafts,
            &workflow,
        )
        .await;
        let candidates = match candidates {
            Ok(candidates) => candidates,
            Err(error) if matches!(error.code, "EXTRACTION_SOURCE_LIMIT" | "WORKFLOW_TIMEOUT") => {
                return self
                    .finish_job(job, revision, "failed", Some(error.code))
                    .await;
            }
            Err(error) => return Err(error.into()),
        };
        job.result.as_mut().unwrap()["candidates"] = json!(candidates);
        job.result.as_mut().unwrap()["generator"] = json!(generator);
        if workflow.expired().is_some() {
            return self
                .finish_job(job, revision, "failed", Some("WORKFLOW_TIMEOUT"))
                .await;
        }
        self.finish_job(job, revision, "succeeded", None).await
    }

    async fn native_extraction(
        &self,
        job: &Job,
        config: &ResolvedConfig,
        workflow: &Deadline,
        trace: &mut Value,
    ) -> Result<String> {
        let plan = job.extraction_plan.as_ref().ok_or(Error("JOB_CORRUPT"))?;
        let native = job.extraction_native.as_ref().ok_or(Error("JOB_CORRUPT"))?;
        let profile = config
            .config()
            .retrieval
            .extraction
            .as_deref()
            .and_then(|s| s.strip_prefix("executor:"))
            .ok_or(Error("CONFIG_CORRUPT"))?;
        let budget = RunBudget::new(
            &job.id,
            &job.request_sha256,
            self.runner_slots.clone(),
            BudgetLimits {
                starts: 1,
                depth: 0,
                ..Default::default()
            },
            workflow.clone(),
        )?;
        let outcome = async {
            let lease = {
                let _guard = self.admission.lock().await;
                self.authorize_job(job)?;
                let max = self.config.pin().resolved().config().runtime.max_concurrent_jobs.min(MAX_RUNNING);
                if MAX_RUNNING - self.runner_slots.available_permits() >= max { return Err(Error("DELEGATION_UNAVAILABLE")); }
                budget.start()?
            };
            let instructions = plan.prompt(&self.storage, &context()?)?;
            let directory = config.config().paths.data_dir.join("runner-work");
            private_dir(&directory)?;
            let (options, runner_timeout) = native.options(config, profile)?;
            let mut runner = native.runner(&directory, &instructions).await?;
            let mut spec = RunSpec::for_extraction(RunOptions { job_id:job.id.clone(), executor_profile:profile.into(), instructions_version:PROMPT_VERSION.into(), runner_timeout, args:ConsultArgs::new(None)? }, lease, job.config_generation)?.with_guard(std::env::current_exe()?);
            spec.native_options = options;
            let lease = spec.lease_handle();
            let mut collector = RunCollector::new(&lease, crate::adapter::MAX_FRAME_BYTES);
            let mut source = SuppliedEvidence;
            let mut handle = {
                let _guard = self.admission.lock().await;
                let (_, current) = self.job(&job.id)?;
                self.authorize_job(&current)?;
                if current.cancel_requested { return Err(Error("CANCELLED")); }
                if self.stopping.load(std::sync::atomic::Ordering::Acquire) { return Err(Error("RUNNER_INTERRUPTED")); }
                runner.start(spec, &mut source).await?
            };
            let run = async {
                while let Some(event) = handle.next().await? { collector.push(event)?; }
                collector.finish(handle.exit_status().await?)
            };
            let result = tokio::select! {
                biased;
                code=self.model_cancelled(&job.id) => Err(Error(if code == "PROVIDER_INTERRUPTED" { "RUNNER_INTERRUPTED" } else { code })),
                _=tokio::time::sleep(workflow.remaining()) => Err(Error("WORKFLOW_TIMEOUT")),
                result=run => result.map_err(Error::from),
            };
            if result.is_err() { handle.cancel(CancelReason::User).await?; }
            drop(handle);
            result
        }.await;
        trace["usage"] = json!(budget.usage()?);
        trace["deadline"] = json!(workflow.trace());
        trace["host"] = json!(native.host);
        outcome
    }
}
