use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::Read;
use std::sync::{Arc, Mutex};

const MAX_PROCESSES: usize = 4096;
const MAX_THREADS: usize = 32768;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Identity {
    parent: u32,
    start: u64,
    zombie: bool,
}

fn read(path: impl AsRef<std::path::Path>) -> Option<String> {
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(65537)
        .read_to_string(&mut text)
        .ok()?;
    (text.len() <= 65536).then_some(text)
}

fn identity(pid: u32) -> Option<Identity> {
    let stat = read(format!("/proc/{pid}/stat"))?;
    let fields: Vec<_> = stat.rsplit_once(") ")?.1.split_whitespace().collect();
    Some(Identity {
        parent: fields.get(1)?.parse().ok()?,
        start: fields.get(19)?.parse().ok()?,
        zombie: matches!(*fields.first()?, "Z" | "X"),
    })
}

fn kib(text: &str, name: &str) -> Option<u64> {
    let mut lines = text.lines().filter_map(|line| line.strip_prefix(name));
    let mut value = lines.next()?.split_whitespace();
    let bytes = value.next()?.parse::<u64>().ok()?.checked_mul(1024)?;
    (value.next() == Some("kB") && value.next().is_none() && lines.next().is_none())
        .then_some(bytes)
}

#[derive(Clone, Debug, Default, Serialize)]
struct Group {
    pids: Vec<u32>,
    rss_bytes: u64,
    pss_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Measurement {
    daemon_group: Group,
    runner_group: Group,
    service_total: Group,
    #[serde(skip)]
    runners: BTreeMap<u64, u64>,
}

impl Measurement {
    pub(crate) fn admits(&self, limits: crate::config::MemoryLimits) -> bool {
        self.daemon_group.rss_bytes < u64::from(limits.daemon_memory_target_mib) * 1024 * 1024
            && self.service_total.rss_bytes
                < u64::from(limits.service_total_budget_mib) * 1024 * 1024
    }
}

#[derive(Debug, Default)]
struct State {
    next: u64,
    reservations: BTreeMap<u64, Allocation>,
    runner_peaks: BTreeMap<String, u64>,
    other_runner_peak: u64,
    daemon_peak: u64,
    runner_peak: u64,
    total_peak: u64,
    samples: u64,
}

#[derive(Debug)]
struct Allocation {
    profile: String,
    bytes: u64,
    root: Option<(u32, Identity)>,
    sampled_peak: u64,
}

#[derive(Debug, Default)]
pub(crate) struct Resources {
    state: Mutex<State>,
    admission: tokio::sync::Mutex<()>,
    #[cfg(test)]
    fixture: Option<std::sync::Mutex<Option<Measurement>>>,
}

#[derive(Debug)]
pub(crate) struct Reservation {
    resources: Arc<Resources>,
    id: u64,
}

tokio::task_local! {
    static NATIVE_RESERVATION: Arc<Reservation>;
}

pub(crate) fn native_reservation() -> Option<Arc<Reservation>> {
    NATIVE_RESERVATION.try_with(Arc::clone).ok()
}

impl Reservation {
    pub(crate) async fn scope<T>(
        self: &Arc<Self>,
        future: impl std::future::Future<Output = T>,
    ) -> T {
        NATIVE_RESERVATION.scope(self.clone(), future).await
    }

    pub(crate) fn register(&self, pid: u32) {
        let owned = identity(pid).filter(|p| p.parent == std::process::id());
        if let Some(allocation) = self
            .resources
            .state
            .lock()
            .unwrap()
            .reservations
            .get_mut(&self.id)
        {
            allocation.root = owned.map(|id| (pid, id));
        }
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        let mut state = self.resources.state.lock().unwrap();
        if let Some(allocation) = state
            .reservations
            .remove(&self.id)
            .filter(|a| a.sampled_peak > 0)
        {
            if state.runner_peaks.len() < 256
                || state.runner_peaks.contains_key(&allocation.profile)
            {
                let peak = state.runner_peaks.entry(allocation.profile).or_default();
                *peak = (*peak).max(allocation.sampled_peak);
            } else {
                state.other_runner_peak = state.other_runner_peak.max(allocation.sampled_peak);
            }
        }
    }
}

impl Resources {
    pub(crate) async fn measure(&self) -> Option<Measurement> {
        let sample = self.read_measurement().await?;
        let mut state = self.state.lock().unwrap();
        state.samples = state.samples.saturating_add(1);
        state.daemon_peak = state.daemon_peak.max(sample.daemon_group.rss_bytes);
        state.runner_peak = state.runner_peak.max(sample.runner_group.rss_bytes);
        state.total_peak = state.total_peak.max(sample.service_total.rss_bytes);
        for (id, bytes) in &sample.runners {
            if let Some(allocation) = state.reservations.get_mut(id) {
                allocation.sampled_peak = allocation.sampled_peak.max(*bytes);
            }
        }
        Some(sample)
    }

    async fn read_measurement(&self) -> Option<Measurement> {
        #[cfg(test)]
        if let Some(fixture) = &self.fixture {
            return fixture.lock().unwrap().clone();
        }
        let roots = self
            .state
            .lock()
            .unwrap()
            .reservations
            .iter()
            .filter_map(|(id, allocation)| {
                allocation
                    .root
                    .map(|(pid, identity)| (pid, (*id, identity)))
            })
            .collect();
        tokio::task::spawn_blocking(move || measure(std::process::id(), &roots))
            .await
            .ok()
            .flatten()
    }

    fn projected(sample: &Measurement, state: &State, daemon_reserve: u64) -> Option<u64> {
        let mut bytes = sample
            .service_total
            .rss_bytes
            .checked_add(daemon_reserve.saturating_sub(sample.daemon_group.rss_bytes))?;
        for (id, allocation) in &state.reservations {
            bytes = bytes.checked_add(
                allocation
                    .bytes
                    .saturating_sub(*sample.runners.get(id).unwrap_or(&0)),
            )?;
        }
        Some(bytes)
    }

    pub(crate) fn admits(
        &self,
        sample: Option<&Measurement>,
        limits: crate::config::MemoryLimits,
    ) -> bool {
        let state = self.state.lock().unwrap();
        let daemon_reserve = if state.reservations.is_empty() {
            0
        } else {
            u64::from(limits.daemon_peak_limit_mib) * 1024 * 1024
        };
        sample.is_some_and(|sample| {
            sample.admits(limits)
                && Self::projected(sample, &state, daemon_reserve).is_some_and(|bytes| {
                    bytes < u64::from(limits.service_total_budget_mib) * 1024 * 1024
                })
        })
    }

    pub(crate) async fn reserve(
        self: &Arc<Self>,
        limits: crate::config::MemoryLimits,
        profile: &str,
        estimate_mib: u32,
    ) -> Option<Arc<Reservation>> {
        let _admission = self.admission.lock().await;
        let sample = self.measure().await?;
        if estimate_mib == 0 || !sample.admits(limits) {
            return None;
        }
        let mut state = self.state.lock().unwrap();
        let estimate = (u64::from(estimate_mib) * 1024 * 1024).max(
            *state
                .runner_peaks
                .get(profile)
                .unwrap_or(&state.other_runner_peak),
        );
        let projected = Self::projected(
            &sample,
            &state,
            u64::from(limits.daemon_peak_limit_mib) * 1024 * 1024,
        )?;
        if state.reservations.len() >= 256
            || projected.checked_add(estimate)?
                > u64::from(limits.service_total_budget_mib) * 1024 * 1024
        {
            return None;
        }
        state.next = state.next.checked_add(1)?;
        let id = state.next;
        state.reservations.insert(
            id,
            Allocation {
                profile: profile.into(),
                bytes: estimate,
                root: None,
                sampled_peak: 0,
            },
        );
        Some(Arc::new(Reservation {
            resources: self.clone(),
            id,
        }))
    }

    pub(crate) async fn can_start(&self, limits: crate::config::MemoryLimits) -> bool {
        self.measure().await.is_some_and(|sample| {
            sample.admits(limits)
                && Self::projected(
                    &sample,
                    &self.state.lock().unwrap(),
                    u64::from(limits.daemon_peak_limit_mib) * 1024 * 1024,
                )
                .is_some_and(|bytes| {
                    bytes <= u64::from(limits.service_total_budget_mib) * 1024 * 1024
                })
        })
    }

    pub(crate) fn status(&self) -> Value {
        let state = self.state.lock().unwrap();
        json!({"active_reservations":state.reservations.len(),
            "reserved_runner_bytes":state.reservations.values().map(|a|a.bytes).sum::<u64>(),
            "sample_count":state.samples,
            "sampled_peak_rss_bytes":(state.samples>0).then(||json!({"daemon_group":state.daemon_peak,"runner_group":state.runner_peak,"service_total":state.total_peak})),
            "completed_runner_sampled_peaks":state.runner_peaks,
            "other_runner_sampled_peak":state.other_runner_peak})
    }

    #[cfg(test)]
    pub(crate) fn fixture() -> Self {
        let resources = Self {
            fixture: Some(std::sync::Mutex::new(None)),
            ..Self::default()
        };
        // In-process service fixtures do not own the test harness's other threads or children.
        resources.set_fixture_sample(Some((1024 * 1024, 0)));
        resources
    }

    #[cfg(test)]
    pub(crate) fn set_fixture_sample(&self, bytes: Option<(u64, u64)>) {
        *self.fixture.as_ref().unwrap().lock().unwrap() =
            bytes.map(|(daemon, runners)| Measurement {
                daemon_group: Group {
                    rss_bytes: daemon,
                    ..Group::default()
                },
                runner_group: Group {
                    rss_bytes: runners,
                    ..Group::default()
                },
                service_total: Group {
                    rss_bytes: daemon.checked_add(runners).unwrap(),
                    ..Group::default()
                },
                runners: BTreeMap::new(),
            });
    }
}

fn measure(root: u32, roots: &BTreeMap<u32, (u64, Identity)>) -> Option<Measurement> {
    let mut pending = vec![(root, None, None)];
    let mut seen = BTreeMap::new();
    let mut daemon = Group::default();
    let mut runners = Group::default();
    let mut threads = 0;
    let mut by_runner = BTreeMap::<u64, u64>::new();
    while let Some((pid, parent, inherited)) = pending.pop() {
        if seen.contains_key(&pid) {
            continue;
        }
        if seen.len() >= MAX_PROCESSES {
            return None;
        }
        let before = identity(pid)?;
        let runner = roots
            .get(&pid)
            .filter(|(_, id)| id.start == before.start)
            .map(|(id, _)| *id)
            .or(inherited);
        if parent.is_some_and(|p| p != before.parent) {
            return None;
        }
        let (rss, pss) = if before.zombie {
            (0, 0)
        } else {
            let rollup = read(format!("/proc/{pid}/smaps_rollup"))?;
            (kib(&rollup, "Rss:")?, kib(&rollup, "Pss:")?)
        };
        // Children belong to threads; scanning only the leader misses Tokio-spawned workers.
        for task in std::fs::read_dir(format!("/proc/{pid}/task")).ok()? {
            threads += 1;
            if threads > MAX_THREADS {
                return None;
            }
            let children = read(task.ok()?.path().join("children"))?;
            for child in children.split_whitespace() {
                if pending.len() + seen.len() >= MAX_PROCESSES {
                    return None;
                }
                pending.push((child.parse().ok()?, Some(pid), runner));
            }
        }
        seen.insert(pid, before);
        let group = if let Some(runner) = runner {
            let bytes = by_runner.entry(runner).or_default();
            *bytes = bytes.checked_add(rss)?;
            &mut runners
        } else {
            &mut daemon
        };
        group.pids.push(pid);
        group.rss_bytes = group.rss_bytes.checked_add(rss)?;
        group.pss_bytes = group.pss_bytes.checked_add(pss)?;
    }
    // Refuse a raced sample rather than count a reused PID or a vanished subtree as zero.
    for (pid, before) in &seen {
        if identity(*pid)? != *before {
            return None;
        }
    }
    runners.pids.sort_unstable();
    let total = Group {
        pids: seen.keys().copied().collect(),
        rss_bytes: daemon.rss_bytes.checked_add(runners.rss_bytes)?,
        pss_bytes: daemon.pss_bytes.checked_add(runners.pss_bytes)?,
    };
    Some(Measurement {
        daemon_group: daemon,
        runner_group: runners,
        service_total: total,
        runners: by_runner,
    })
}

pub(crate) fn sample(measurement: Option<Measurement>) -> Value {
    let mut value = match measurement {
        Some(value) => json!(value),
        None => json!({"daemon_group":null,"runner_group":null,"service_total":null,
            "error":"RESOURCE_MEASUREMENT_UNAVAILABLE"}),
    };
    value["complete"] = json!(value.get("error").is_none());
    value["kind"] = json!("on_demand_procfs");
    value["sampled_at_ms"] = json!(crate::ipc::now_ms().ok());
    // /proc children and memory reads are not an atomic census or a peak measurement.
    value["atomic"] = json!(false);
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> crate::config::MemoryLimits {
        crate::config::MemoryLimits {
            daemon_memory_target_mib: 512,
            daemon_peak_limit_mib: 640,
            service_total_budget_mib: 2048,
        }
    }

    #[tokio::test]
    async fn concurrent_worker_reservations_keep_headroom_and_survive_budget_lowering() {
        let resources = Arc::new(Resources::fixture());
        let (first, second) = tokio::join!(
            resources.reserve(limits(), "a", 1024),
            resources.reserve(limits(), "b", 1024)
        );
        assert_ne!(first.is_some(), second.is_some());
        let first = first.or(second).unwrap();
        let retained = first.clone();
        let lower = crate::config::MemoryLimits {
            service_total_budget_mib: 1024,
            ..limits()
        };
        assert!(!resources.can_start(lower).await);
        assert!(resources.reserve(lower, "c", 1).await.is_none());
        drop(first);
        assert_eq!(resources.status()["active_reservations"], 1);
        drop(retained);
        assert_eq!(resources.status()["active_reservations"], 0);
        assert!(resources.reserve(lower, "c", 128).await.is_some());
    }

    #[tokio::test]
    async fn observed_worker_growth_keeps_other_reservations_and_informs_later_admission() {
        let resources = Arc::new(Resources::fixture());
        let first = resources.reserve(limits(), "a", 512).await.unwrap();
        let second = resources.reserve(limits(), "b", 512).await.unwrap();
        resources.set_fixture_sample(Some((1024 * 1024, 1024 * 1024 * 1024)));
        resources
            .fixture
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .runners
            .insert(first.id, 1024 * 1024 * 1024);
        let sample = resources.measure().await.unwrap();
        assert!(!resources.admits(Some(&sample), limits()));
        assert!(!resources.can_start(limits()).await);
        assert!(resources.reserve(limits(), "c", 1).await.is_none());
        drop(first);
        drop(second);
        resources.set_fixture_sample(Some((1024 * 1024, 0)));
        let next = resources.reserve(limits(), "a", 128).await.unwrap();
        assert_eq!(
            resources.status()["reserved_runner_bytes"],
            1024 * 1024 * 1024u64
        );
        let lower = crate::config::MemoryLimits {
            service_total_budget_mib: 1536,
            ..limits()
        };
        drop(next);
        assert!(resources.reserve(lower, "a", 128).await.is_none());
    }

    #[tokio::test]
    async fn native_probe_context_is_scoped_to_the_owning_future() {
        let left = Arc::new(Resources::fixture())
            .reserve(limits(), "a", 1024)
            .await
            .unwrap();
        let right = Arc::new(Resources::fixture())
            .reserve(limits(), "b", 1024)
            .await
            .unwrap();
        let (a, b) = tokio::join!(
            left.scope(async {
                tokio::task::yield_now().await;
                native_reservation().unwrap()
            }),
            right.scope(async {
                tokio::task::yield_now().await;
                native_reservation().unwrap()
            })
        );
        assert!(Arc::ptr_eq(&a, &left));
        assert!(Arc::ptr_eq(&b, &right));
        assert!(native_reservation().is_none());
    }

    #[test]
    fn memory_samples_refuse_unknown_processes_and_malformed_or_overflowing_units() {
        let failed = sample(measure(u32::MAX, &BTreeMap::new()));
        assert_eq!(failed["complete"], false);
        assert!(failed["service_total"].is_null());
        assert_eq!(failed["error"], "RESOURCE_MEASUREMENT_UNAVAILABLE");
        assert_eq!(kib("Rss: 12 kB\nPss: 8 kB\n", "Rss:"), Some(12288));
        for text in [
            "",
            "Rss: 8 bytes",
            "Rss: 8 kB extra",
            "Rss: 8 kB\nRss: 9 kB",
            "Rss: 18446744073709551615 kB",
        ] {
            assert_eq!(kib(text, "Rss:"), None, "{text}");
        }
    }
}
