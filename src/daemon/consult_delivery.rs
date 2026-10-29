use super::*;
use crate::domain::{OriginalRange, ReadResult, RecallReceipt};

impl Service {
    fn consult_job(&self, id: &str) -> Result<Job> {
        let (_, job) = self.job(id)?;
        if job.consult_plan.is_none() {
            return Err(Error("INVALID_ARGUMENT"));
        }
        self.authorize_job(&job)?;
        Ok(job)
    }

    async fn read_consult_receipt(
        &self,
        job: &Job,
        full: bool,
    ) -> Result<ReadResult<RecallReceipt>> {
        if job.state != "succeeded" {
            return Err(Error("RESULT_UNAVAILABLE"));
        }
        let trace = job.result.as_ref().ok_or(Error("JOB_CORRUPT"))?;
        self.authorize_consult_result(trace)?;
        let receipt = if full && !trace["detail_receipt_id"].is_null() {
            &trace["detail_receipt_id"]
        } else {
            &trace["receipt_id"]
        }
        .as_str()
        .ok_or(Error("JOB_CORRUPT"))?;
        Ok(Domain::new(&self.storage)
            .recall_receipt(&self.job_read_context(job)?, receipt)
            .await?)
    }

    pub(in crate::daemon) async fn show_consult(&self, id: &str) -> Result<Value> {
        let _guard = self.admission.lock().await;
        let job = self.consult_job(id)?;
        if !job.terminal() {
            return Ok(json!({"status":"pending","job_id":id,"receipt_id":null}));
        }
        if job.state != "succeeded" {
            return Ok(json!({"status":"error","job_id":id,"receipt_id":null,
                "error":{"code":job.error.as_deref().ok_or(Error("JOB_CORRUPT"))?}}));
        }
        let receipt = self.read_consult_receipt(&job, false).await?;
        let context = self.job_read_context(&job)?;
        let domain = Domain::new(&self.storage);
        let now = i64::try_from(ipc::now_ms()?).map_err(|_| Error("CLOCK_INVALID"))?;
        for reference in &receipt.data.record.selected {
            let crate::domain::MemoryState::Active(current) =
                domain.get(&context, &reference.key.id, None).await?.data
            else {
                return Err(Error("STALE_RECALL"));
            };
            if current.memory.revision != *reference || !current.memory.record.draft.valid_at(now) {
                return Err(Error("STALE_RECALL"));
            }
        }
        Ok(serde_json::from_str(&receipt.data.output)?)
    }

    pub(in crate::daemon) async fn consult_receipt(&self, id: &str, full: bool) -> Result<Value> {
        let _guard = self.admission.lock().await;
        let job = self.consult_job(id)?;
        Ok(serde_json::to_value(
            self.read_consult_receipt(&job, full).await?,
        )?)
    }

    pub(in crate::daemon) async fn consult_original(
        &self,
        id: &str,
        source: usize,
        start: usize,
        request_id: &str,
    ) -> Result<Value> {
        let _guard = self.admission.lock().await;
        let job = self.consult_job(id)?;
        let receipt = self.read_consult_receipt(&job, false).await?;
        let span = receipt
            .data
            .record
            .sources
            .get(source)
            .ok_or(Error("INVALID_RANGE"))?;
        let start = span
            .start
            .checked_add(start)
            .filter(|s| *s < span.end)
            .ok_or(Error("INVALID_RANGE"))?;
        let config = self.retrieval_snapshot(job.config_generation)?;
        let original = Domain::new(&self.storage)
            .original_range(
                &self.job_read_context(&job)?,
                &OriginalRange {
                    event_id: span.event_id.clone(),
                    revision: span.revision,
                    start,
                    end: span.end,
                },
                &format!(
                    "consult-expand/{}",
                    hash(&serde_json::to_vec(&(id, source, start, request_id))?)
                ),
                config.resolved().config().retrieval.max_original_chars,
                config.generation(),
            )
            .await?;
        if original.reference.content_sha256 != span.content_sha256 {
            return Err(Error("SOURCE_CHANGED"));
        }
        Ok(serde_json::to_value(original)?)
    }
}
