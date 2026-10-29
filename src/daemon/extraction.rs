use super::*;
use crate::config::{ResolvedConfig, deadline::Deadline};
use crate::domain::Generator;
use crate::extraction::{self, PROMPT_VERSION, Plan};

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
        let (plan, config, call) = {
            let _guard = self.admission.lock().await;
            let (_, job) = self.job(&id)?;
            self.authorize_job(&job)?;
            let config = self.retrieval_snapshot(job.config_generation)?;
            if config.resolved().digest() != job.config_digest {
                return Err(Error("CONFIG_CORRUPT"));
            }
            let plan = job.extraction_plan.ok_or(Error("JOB_CORRUPT"))?;
            let call = self.extraction_call(&plan, config.resolved())?;
            (plan, config, call)
        };
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
        let _guard = self.admission.lock().await;
        let (revision, mut job) = self.job(&id)?;
        if job.state != "running" || job.lease_owner.as_deref() != Some(&self.instance) {
            return Err(Error("LEASE_LOST"));
        }
        job.result = Some(json!({"prompt_version":PROMPT_VERSION,"trace":outcome.trace}));
        if job.cancel_requested {
            return self
                .finish_job(job, revision, "cancelled", Some("CANCELLED"))
                .await;
        }
        if let Some(reason) = outcome.error {
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
            let text = outcome
                .output
                .as_ref()
                .and_then(|v| v["text"].as_str())
                .ok_or(Error("EXTRACTION_OUTPUT_INVALID"))?;
            Ok::<_, Error>(extraction::parse(&plan, text, &records)?)
        })();
        let drafts = match prepared {
            Ok(drafts) => drafts,
            Err(error) => {
                return self
                    .finish_job(job, revision, "failed", Some(error.0))
                    .await;
            }
        };
        let generator = Generator {
            runner: format!("text/{}", call.profile),
            model: config.resolved().config().models[&call.profile]
                .model
                .clone(),
            prompt_version: PROMPT_VERSION.into(),
        };
        let candidates = extraction::publish(
            &self.storage,
            &plan,
            &id,
            job.config_generation,
            generator.clone(),
            drafts,
        )
        .await?;
        job.result.as_mut().unwrap()["candidates"] = json!(candidates);
        job.result.as_mut().unwrap()["generator"] = json!(generator);
        self.finish_job(job, revision, "succeeded", None).await
    }
}
