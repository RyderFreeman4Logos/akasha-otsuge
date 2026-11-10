use super::Service;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::Read;

const MAX_PROCESSES: usize = 4096;
const MAX_THREADS: usize = 32768;

#[derive(Clone, Copy, Eq, PartialEq)]
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

#[derive(Default, Serialize)]
struct Group {
    pids: Vec<u32>,
    rss_bytes: u64,
    pss_bytes: u64,
}

fn measure(root: u32) -> Option<Value> {
    let mut pending = vec![(root, None)];
    let mut seen = BTreeMap::new();
    let mut daemon = Group::default();
    let mut runners = Group::default();
    let mut threads = 0;
    while let Some((pid, parent)) = pending.pop() {
        if seen.contains_key(&pid) {
            continue;
        }
        if seen.len() >= MAX_PROCESSES {
            return None;
        }
        let before = identity(pid)?;
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
                pending.push((child.parse().ok()?, Some(pid)));
            }
        }
        seen.insert(pid, before);
        let group = if pid == root {
            &mut daemon
        } else {
            &mut runners
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
    Some(json!({"daemon_group":daemon,"runner_group":runners,"service_total":total}))
}

fn sample(root: u32) -> Value {
    let mut value = match measure(root) {
        Some(value) => value,
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

impl Service {
    pub(super) async fn resource_status(&self) -> Value {
        let config = self.config.pin();
        let requested = config.resolved().config().runtime.memory_limits();
        let index = match self.search_index.try_lock() {
            Ok(index) => {
                json!({"inspected":true,"open_budget_mib":index.as_ref().map(|i|i.budget_mib)})
            }
            Err(_) => json!({"inspected":false,"open_budget_mib":null}),
        };
        let measurement = tokio::task::spawn_blocking(|| sample(std::process::id()))
            .await
            .unwrap_or_else(
                |_| json!({"complete":false,"error":"RESOURCE_MEASUREMENT_UNAVAILABLE"}),
            );
        json!({"config_generation":config.generation(),"requested":requested,
            "storage_open_budget_mib":self.storage.budget_mib(),
            "storage_reopen_required":self.storage.budget_mib()!=requested.daemon_memory_target_mib,
            "index":index,"measurement":measurement,
            "enforcement":{"software_memory_admission":false,"managed_cgroup_limits":false}})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_samples_refuse_unknown_processes_and_malformed_or_overflowing_units() {
        let failed = sample(u32::MAX);
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
