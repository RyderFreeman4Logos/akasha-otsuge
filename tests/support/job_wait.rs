use serde_json::Value;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct JobWait {
    queue: Option<u64>,
    execution: Option<u64>,
    end: Option<Instant>,
}

impl JobWait {
    pub(super) fn check(&mut self, job: &Value) {
        self.check_at(job, Instant::now(), akasha_otsuge::ipc::now_ms().unwrap());
    }

    fn check_at(&mut self, job: &Value, now: Instant, wall_ms: u64) {
        let queue = job["queue_deadline_ms"].as_u64().unwrap();
        assert_eq!(
            *self.queue.get_or_insert(queue),
            queue,
            "queue deadline changed"
        );
        let execution = job["execution_deadline_ms"].as_u64();
        if let Some(original) = self.execution {
            assert_eq!(execution, Some(original), "execution deadline changed");
        }
        if let Some(deadline) = execution {
            assert_eq!(
                job["started_at_ms"]
                    .as_u64()
                    .unwrap()
                    .checked_add(job["execution_budget_ms"].as_u64().unwrap()),
                Some(deadline),
                "invalid admitted execution clock"
            );
        }
        if self.end.is_none() || self.execution.is_none() && execution.is_some() {
            // Allow the bounded IPC interval for terminal publication, without renewing the job.
            let cutoff = execution
                .unwrap_or(queue)
                .checked_add(
                    akasha_otsuge::ipc::TRANSPORT_TIMEOUT
                        .as_millis()
                        .try_into()
                        .unwrap(),
                )
                .unwrap();
            self.end = Some(now + Duration::from_millis(cutoff.saturating_sub(wall_ms)));
        }
        self.execution = execution;
        assert!(
            now < self.end.unwrap(),
            "job exceeded its admitted deadline and terminal grace: {job}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn job(start: Option<u64>) -> Value {
        json!({"queue_deadline_ms":301_000,"started_at_ms":start,
            "execution_budget_ms":120_000,"execution_deadline_ms":start.map(|s| s+120_000)})
    }

    #[test]
    fn capture_wait_uses_the_remaining_admitted_clock_after_restart() {
        let now = Instant::now();
        let mut wait = JobWait::default();
        wait.check_at(&job(Some(1_000)), now, 41_000);
        wait.check_at(&job(Some(1_000)), now + Duration::from_secs(89), 130_000);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                wait.check_at(&job(Some(1_000)), now + Duration::from_secs(90), 131_000);
            }))
            .is_err()
        );
    }

    #[test]
    fn capture_wait_switches_once_from_queue_to_execution() {
        let now = Instant::now();
        let mut wait = JobWait::default();
        wait.check_at(&job(None), now, 1_000);
        wait.check_at(&job(Some(5_000)), now + Duration::from_secs(4), 5_000);
        wait.check_at(&job(Some(5_000)), now + Duration::from_secs(133), 134_000);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                wait.check_at(&job(Some(5_000)), now + Duration::from_secs(134), 135_000);
            }))
            .is_err()
        );
    }

    #[test]
    fn capture_wait_never_renews_on_poll_or_wall_clock_reversal() {
        let now = Instant::now();
        let mut wait = JobWait::default();
        wait.check_at(&job(Some(1_000)), now, 1_000);
        wait.check_at(&job(Some(1_000)), now + Duration::from_secs(129), 0);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                wait.check_at(&job(Some(1_000)), now + Duration::from_secs(130), 0);
            }))
            .is_err()
        );
    }

    #[test]
    fn capture_wait_rejects_changed_or_already_expired_clocks() {
        let now = Instant::now();
        let mut wait = JobWait::default();
        wait.check_at(&job(Some(1_000)), now, 1_000);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                wait.check_at(&job(Some(2_000)), now, 2_000);
            }))
            .is_err()
        );
        assert!(
            std::panic::catch_unwind(|| {
                JobWait::default().check_at(&job(None), now, 311_000);
            })
            .is_err()
        );
        assert!(
            std::panic::catch_unwind(|| {
                JobWait::default().check_at(&job(Some(1_000)), now, 131_000);
            })
            .is_err()
        );
    }
}
