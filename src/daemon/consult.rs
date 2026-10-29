use super::*;
use crate::adapter::{BudgetLimits, CapturedContext, ConsultArgs, Invocation, RunBudget};
use crate::config::{Host, deadline::Deadline};
use crate::domain::{Coverage, RecallDraft, RecallStatus, RetrievalMode};
#[path = "consult_research.rs"]
mod research;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::daemon) struct NativeExecutor {
    pub host: Host,
    pub binary: PathBuf,
    pub home: PathBuf,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Plan {
    invocation_id: String,
    principal: String,
    invocation_revision: u64,
    fence: String,
    pub(super) config_generation: u64,
    native: NativeExecutor,
    args: ConsultArgs,
}

impl Service {
    pub(super) fn authorize_consult_result(&self, trace: &Value) -> Result<()> {
        if !trace["workspace"].is_null() {
            let proof: crate::adapter::WorkspaceProof =
                serde_json::from_value(trace["workspace"].clone())
                    .map_err(|_| Error("JOB_CORRUPT"))?;
            proof.authorize(&self.config)?;
        }
        Ok(())
    }
    fn authorize_native(&self, native: &NativeExecutor) -> Result<()> {
        if self.allowed_path(&native.home)? != native.home || !native.home.is_dir() {
            return Err(Error("READ_DENIED"));
        }
        Ok(())
    }
    pub(in crate::daemon) async fn submit_consult(
        self: &Arc<Self>,
        invocation: Invocation,
        native: NativeExecutor,
        args: ConsultArgs,
    ) -> Result<Value> {
        let id = format!("consult/{}", hash(invocation.id().as_bytes()));
        let plan = Plan {
            invocation_id: invocation.id().into(),
            principal: invocation.diagnostic().principal.clone(),
            invocation_revision: invocation.revision(),
            fence: invocation.fence_hash(),
            config_generation: invocation.diagnostic().config_generation,
            native,
            args: ConsultArgs::new(args.hint)?,
        };
        self.submit_job(id, None, None, None, None, Some(plan))
            .await
    }

    pub(super) fn consult_invocation(&self, plan: &Plan) -> Result<Invocation> {
        let invocation =
            Invocation::load_verified(&self.storage, &plan.invocation_id, &plan.principal)?;
        if invocation.revision() != plan.invocation_revision
            || invocation.fence_hash() != plan.fence
            || invocation.diagnostic().config_generation != plan.config_generation
        {
            return Err(Error("BINDING_CONFLICT"));
        }
        Ok(invocation)
    }

    pub(super) fn job_read_context(&self, job: &Job) -> Result<ReadContext> {
        match &job.consult_plan {
            Some(plan) => {
                let invocation = self.consult_invocation(plan)?;
                Ok(ReadContext::from_verified_binding(
                    &invocation.diagnostic().principal,
                    invocation.id(),
                    &job.id,
                )?)
            }
            None => context(),
        }
    }

    async fn consult_result(&self, id: &str, workflow: &Deadline) -> Result<Value> {
        let (plan, config, invocation) = {
            let _guard = self.admission.lock().await;
            let (_, job) = self.job(id)?;
            self.authorize_job(&job)?;
            let config = self.retrieval_snapshot(job.config_generation)?;
            if config.resolved().digest() != job.config_digest {
                return Err(Error("CONFIG_CORRUPT"));
            }
            let plan = job.consult_plan.ok_or(Error("JOB_CORRUPT"))?;
            self.authorize_native(&plan.native)?;
            let invocation = self.consult_invocation(&plan)?;
            (plan, config, invocation)
        };
        let settings = &config.resolved().config().consult;
        if settings.executor != "caller" {
            return Err(Error("EXECUTOR_UNAVAILABLE"));
        }
        let budget = RunBudget::new(
            invocation.id(),
            &invocation.fence_hash(),
            self.runner_slots.clone(),
            BudgetLimits {
                starts: settings.max_runner_starts,
                depth: settings.max_delegation_depth,
                ..BudgetLimits::default()
            },
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
        let mut context = research::Research::new(
            self,
            id,
            invocation.clone(),
            config.clone(),
            workflow,
            plan.native,
            budget.clone(),
        )?;
        let outcome = context.run(lease, plan.args).await;
        if outcome.is_err() {
            budget.cancel();
        }
        let answer = outcome?;
        let mut trace = context.trace()?;
        let sources = context.sources;
        let mut gaps = context.gaps;
        if sources.is_empty() && context.selected.is_empty() {
            gaps.push("no_evidence".to_owned());
        }
        if invocation.coverage() != crate::adapter::FenceCoverage::CompleteToFence {
            gaps.push("history_gap".to_owned());
        }
        let status = if gaps.is_empty() {
            RecallStatus::Found
        } else {
            RecallStatus::Incomplete
        };
        let scope = invocation.diagnostic().scope.clone();
        let output = serde_json::to_string(&json!({"status":status,"answer":answer,"scope":scope,
            "evidence":sources,"memories":context.selected,"code":context.code,"conflicts":[],"gaps":gaps}))?;
        if output.chars().count() > settings.max_result_chars {
            return Err(Error("OUTPUT_LIMIT"));
        }
        let receipt_id = format!("consult-result/{}", hash(id.as_bytes()));
        let _guard = self.admission.lock().await;
        let (_, job) = self.job(id)?;
        self.authorize_job(&job)?;
        if job.cancel_requested {
            return Err(Error("CANCELLED"));
        }
        if workflow.expired().is_some() {
            return Err(Error("WORKFLOW_TIMEOUT"));
        }
        self.authorize_consult_result(&trace)?;
        Domain::new(&self.storage)
            .prepare_recall(
                &self.job_read_context(&job)?,
                &RecallDraft {
                    request_id: receipt_id.clone(),
                    output,
                    scope,
                    read_seq: context.read_seq,
                    retrieval_mode: RetrievalMode::CurrentState,
                    selected: context.selected,
                    sources,
                    status,
                    coverage: Coverage {
                        complete: gaps.is_empty(),
                        gaps,
                    },
                    truncation: None,
                    config_generation: config.generation(),
                    index_generation: context.index_generation,
                },
            )
            .await?;
        trace["receipt_id"] = json!(receipt_id);
        Ok(trace)
    }

    pub(super) async fn execute_consult(
        self: &Arc<Self>,
        id: String,
        workflow: Deadline,
    ) -> Result<()> {
        let outcome = self.consult_result(&id, &workflow).await;
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
            Ok(value) if workflow.expired().is_none() => {
                job.result = Some(value);
                self.finish_job(job, revision, "succeeded", None).await
            }
            Ok(_) => {
                self.finish_job(job, revision, "failed", Some("WORKFLOW_TIMEOUT"))
                    .await
            }
            Err(error) => {
                self.finish_job(job, revision, "failed", Some(error.0))
                    .await
            }
        }
    }
}
