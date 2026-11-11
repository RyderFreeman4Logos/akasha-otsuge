use crate::config::MemoryLimits;
use rustix::fs::{AtFlags, Mode, OFlags, mkdirat, open, openat, unlinkat};
use serde::Serialize;
use serde_json::{Value, json};
use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::os::unix::fs::{FileExt, MetadataExt};
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid owned cgroup")
}

fn file(directory: &File, name: &str, flags: OFlags) -> io::Result<File> {
    Ok(openat(
        directory,
        name,
        flags | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )?
    .into())
}

fn text(file: &File) -> io::Result<String> {
    let mut bytes = vec![0; 65537];
    let n = file.read_at(&mut bytes, 0)?;
    if n > 65536 {
        return Err(invalid());
    }
    bytes.truncate(n);
    String::from_utf8(bytes).map_err(|_| invalid())
}

fn number(file: &File) -> io::Result<u64> {
    text(file)?.trim().parse().map_err(|_| invalid())
}

fn ceiling(file: &File) -> io::Result<Option<u64>> {
    let value = text(file)?;
    if value.trim() == "max" {
        Ok(None)
    } else {
        value.trim().parse().map(Some).map_err(|_| invalid())
    }
}

fn membership(pid: u32) -> io::Result<PathBuf> {
    let value = super::read(format!("/proc/{pid}/cgroup")).ok_or_else(invalid)?;
    let mut rows = value.lines().filter_map(|line| line.strip_prefix("0::"));
    let path = Path::new(rows.next().ok_or_else(invalid)?);
    if rows.next().is_some()
        || !path.is_absolute()
        || path
            .components()
            .any(|p| !matches!(p, Component::RootDir | Component::Normal(_)))
    {
        return Err(invalid());
    }
    Ok(Path::new("/sys/fs/cgroup").join(path.strip_prefix("/").map_err(|_| invalid())?))
}

#[derive(Debug)]
struct Group {
    path: PathBuf,
    directory: File,
    current: File,
    peak: Option<File>,
    high: File,
    maximum: File,
    events: File,
}

#[derive(Serialize)]
struct Snapshot {
    path: PathBuf,
    current_bytes: u64,
    peak_bytes: Option<u64>,
    high_bytes: Option<u64>,
    max_bytes: Option<u64>,
    oom_kills: u64,
}

impl Group {
    fn open(path: PathBuf) -> io::Result<Self> {
        let directory: File = open(
            &path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )?
        .into();
        if directory.metadata()?.uid() != rustix::process::geteuid().as_raw()
            || rustix::fs::fstatfs(&directory)?.f_type != 0x63677270
        {
            return Err(invalid());
        }
        let peak = match file(&directory, "memory.peak", OFlags::RDONLY) {
            Ok(file) => Some(file),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        Ok(Self {
            current: file(&directory, "memory.current", OFlags::RDONLY)?,
            peak,
            high: file(&directory, "memory.high", OFlags::RDWR)?,
            maximum: file(&directory, "memory.max", OFlags::RDWR)?,
            events: file(&directory, "memory.events", OFlags::RDONLY)?,
            directory,
            path,
        })
    }

    fn snapshot(&self) -> io::Result<Snapshot> {
        let events = text(&self.events)?;
        let mut kills = events.lines().filter_map(|s| s.strip_prefix("oom_kill "));
        let oom_kills = kills
            .next()
            .ok_or_else(invalid)?
            .parse()
            .map_err(|_| invalid())?;
        if kills.next().is_some() {
            return Err(invalid());
        }
        Ok(Snapshot {
            path: self.path.clone(),
            current_bytes: number(&self.current)?,
            peak_bytes: self.peak.as_ref().map(number).transpose()?,
            high_bytes: ceiling(&self.high)?,
            max_bytes: ceiling(&self.maximum)?,
            oom_kills,
        })
    }

    fn set(&self, high: u64, maximum: u64) -> io::Result<()> {
        // Raise max first; lower high first. Callers already proved usage fits.
        if ceiling(&self.maximum)?.is_some_and(|old| maximum > old) {
            self.maximum
                .write_all_at(maximum.to_string().as_bytes(), 0)?;
        }
        if ceiling(&self.high)? != Some(high) {
            self.high.write_all_at(high.to_string().as_bytes(), 0)?;
        }
        if ceiling(&self.maximum)? != Some(maximum) {
            self.maximum
                .write_all_at(maximum.to_string().as_bytes(), 0)?;
        }
        if ceiling(&self.high)? != Some(high) || ceiling(&self.maximum)? != Some(maximum) {
            return Err(invalid());
        }
        Ok(())
    }
}

#[derive(Debug)]
pub(super) struct Managed {
    unit: String,
    root: Group,
    daemon: Group,
    runner: Group,
    effective: Option<MemoryLimits>,
    initial_rss_bytes: u64,
}

impl Managed {
    pub(super) async fn create() -> io::Result<Self> {
        let unit = format!(
            "akasha-otsuge-{}-{}.scope",
            std::process::id(),
            crate::ipc::now_ms().map_err(|_| invalid())?
        );
        let mut command = tokio::process::Command::new("/usr/bin/busctl");
        command
            .args([
                "--user",
                "--timeout=3",
                "call",
                "org.freedesktop.systemd1",
                "/org/freedesktop/systemd1",
                "org.freedesktop.systemd1.Manager",
                "StartTransientUnit",
                "ssa(sv)a(sa(sv))",
                &unit,
                "fail",
                "4",
                "PIDs",
                "au",
                "1",
                &std::process::id().to_string(),
                "Delegate",
                "b",
                "true",
                "MemoryAccounting",
                "b",
                "true",
                "CollectMode",
                "s",
                "inactive-or-failed",
                "0",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .kill_on_drop(true);
        let mut child =
            crate::adapter::native::OwnedChild::new(command.spawn()?).map_err(|_| invalid())?;
        let finished = tokio::time::timeout(
            Duration::from_secs(5),
            crate::adapter::native::wait_for_exit(child.child()),
        )
        .await;
        let status = child.reap().wait().await;
        if !matches!(finished, Ok(Ok(()))) || !status.is_some_and(|s| s.success()) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "user delegation unavailable",
            ));
        }
        tokio::task::spawn_blocking(move || {
            // StartTransientUnit queues a job; its reply is not membership readiness.
            let end = Instant::now() + Duration::from_secs(3);
            let root = loop {
                let path = membership(std::process::id())?;
                if path.file_name().is_some_and(|name| name == unit.as_str())
                    && path.metadata()?.uid() == rustix::process::geteuid().as_raw()
                {
                    let group = Group::open(path)?;
                    file(&group.directory, "cgroup.kill", OFlags::WRONLY)?;
                    break group;
                }
                if Instant::now() >= end {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "delegation readiness timeout",
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
            };
            let initial_rss_bytes = super::read("/proc/self/smaps_rollup")
                .and_then(|s| super::kib(&s, "Rss:"))
                .ok_or_else(invalid)?;
            mkdirat(&root.directory, "daemon", Mode::from_raw_mode(0o700))?;
            mkdirat(&root.directory, "runner", Mode::from_raw_mode(0o700))?;
            let daemon_dir = file(
                &root.directory,
                "daemon",
                OFlags::RDONLY | OFlags::DIRECTORY,
            )?;
            file(&daemon_dir, "cgroup.procs", OFlags::WRONLY)?.write_all_at(b"0", 0)?;
            if !text(&file(&root.directory, "cgroup.procs", OFlags::RDONLY)?)?
                .trim()
                .is_empty()
            {
                return Err(invalid());
            }
            file(&root.directory, "cgroup.subtree_control", OFlags::WRONLY)?
                .write_all_at(b"+memory", 0)?;
            let daemon = Group::open(root.path.join("daemon"))?;
            let runner = Group::open(root.path.join("runner"))?;
            file(&runner.directory, "cgroup.subtree_control", OFlags::WRONLY)?
                .write_all_at(b"+memory", 0)?;
            Ok(Self {
                unit,
                root,
                daemon,
                runner,
                effective: None,
                initial_rss_bytes,
            })
        })
        .await
        .map_err(|_| invalid())?
    }

    fn values(&self, limits: MemoryLimits) -> [(&Group, u64, u64); 3] {
        let target = u64::from(limits.daemon_memory_target_mib) * 1024 * 1024;
        let peak = u64::from(limits.daemon_peak_limit_mib) * 1024 * 1024;
        let total = u64::from(limits.service_total_budget_mib) * 1024 * 1024;
        [
            (&self.daemon, target, peak),
            (&self.runner, total - peak, total - peak),
            (&self.root, total, total),
        ]
    }

    pub(super) fn apply(&mut self, limits: MemoryLimits, drained: bool) -> io::Result<()> {
        if !drained {
            for (group, high, maximum) in self.values(limits) {
                if ceiling(&group.high)?.is_none_or(|old| high < old)
                    || ceiling(&group.maximum)?.is_none_or(|old| maximum < old)
                {
                    return Ok(());
                }
            }
        }
        let rss = super::read("/proc/self/smaps_rollup")
            .and_then(|s| super::kib(&s, "Rss:"))
            .ok_or_else(invalid)?;
        if rss >= u64::from(limits.daemon_memory_target_mib) * 1024 * 1024 {
            return Ok(());
        }
        let values = self.values(limits);
        for (group, high, _) in &values {
            if number(&group.current)? > *high {
                return Ok(());
            }
        }
        // Root capacity rises before children; decreases follow their readbacks.
        let total = u64::from(limits.service_total_budget_mib) * 1024 * 1024;
        if ceiling(&self.root.maximum)?.is_some_and(|old| total > old) {
            self.root.set(total, total)?;
        }
        for (group, high, maximum) in values {
            group.set(high, maximum)?;
        }
        self.effective = Some(limits);
        Ok(())
    }

    pub(super) fn runner(&self, id: u64) -> io::Result<Run> {
        let name = format!("run-{id:016x}");
        let parent = self.runner.directory.try_clone()?;
        mkdirat(&parent, name.as_str(), Mode::from_raw_mode(0o700))?;
        let path = self.runner.path.join(&name);
        let result = (|| {
            let group = Group::open(path)?;
            let events = file(&group.directory, "cgroup.events", OFlags::RDONLY)?;
            let kill = file(&group.directory, "cgroup.kill", OFlags::WRONLY)?;
            Ok(Run {
                group,
                parent: parent.try_clone()?,
                name: name.clone(),
                events,
                kill,
            })
        })();
        if result.is_err() {
            let _ = unlinkat(&parent, name.as_str(), AtFlags::REMOVEDIR);
        }
        result
    }

    pub(super) fn status(&self, requested: MemoryLimits) -> io::Result<Value> {
        let daemon = self.daemon.snapshot()?;
        let runner = self.runner.snapshot()?;
        let total = self.root.snapshot()?;
        let enforced = self.effective.is_some_and(|limits| self.matches(limits));
        Ok(
            json!({"available":true,"complete":true,"unit":self.unit,"path":self.root.path,
            "daemon_group":daemon,"runner_group":runner,"service_total":total,
            "initial_rss_bytes":self.initial_rss_bytes,"preexisting_charges_migrated":false,
            "effective":self.effective.filter(|_|enforced),"pending":self.effective!=Some(requested)||!enforced,
            "enforced":enforced}),
        )
    }

    fn matches(&self, limits: MemoryLimits) -> bool {
        self.effective == Some(limits) && {
            self.values(limits).iter().all(|(group, high, maximum)| {
                ceiling(&group.high).ok() == Some(Some(*high))
                    && ceiling(&group.maximum).ok() == Some(Some(*maximum))
            })
        }
    }
}

#[derive(Debug)]
pub(super) struct Run {
    group: Group,
    parent: File,
    name: String,
    events: File,
    kill: File,
}

impl Run {
    pub(super) fn command(&self, command: Command) -> io::Result<Command> {
        let owner = std::process::id();
        let start = super::identity(owner).ok_or_else(invalid)?.start;
        // Called before each adapter applies its environment, directory and stdio.
        let mut wrapped = Command::new("/proc/self/exe");
        wrapped
            .args([
                OsString::from("__native-runner"),
                OsString::from("--cgroup-exec"),
                owner.to_string().into(),
                start.to_string().into(),
                self.group.path.as_os_str().to_owned(),
            ])
            .arg(command.get_program())
            .args(command.get_args());
        Ok(wrapped)
    }

    pub(super) fn empty(&self) -> io::Result<bool> {
        let events = text(&self.events)?;
        match events.lines().find_map(|s| s.strip_prefix("populated ")) {
            Some("0") => Ok(true),
            Some("1") => Ok(false),
            _ => Err(invalid()),
        }
    }

    pub(super) fn kill(&self) -> io::Result<()> {
        self.kill.write_all_at(b"1", 0)
    }

    pub(super) fn peak(&self) -> Option<u64> {
        self.group.peak.as_ref().and_then(|file| number(file).ok())
    }

    pub(super) fn remove(&self) -> io::Result<()> {
        Ok(unlinkat(
            &self.parent,
            self.name.as_str(),
            AtFlags::REMOVEDIR,
        )?)
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        // Also reclaim a still-empty leaf if reservation preparation was cancelled.
        if self.empty().unwrap_or(false) {
            let _ = self.remove();
        }
    }
}

pub(super) fn exec(args: &[OsString]) -> io::Result<u8> {
    if args.len() < 4 {
        return Err(invalid());
    }
    let owner: u32 = args[0]
        .to_str()
        .and_then(|s| s.parse().ok())
        .ok_or_else(invalid)?;
    let start: u64 = args[1]
        .to_str()
        .and_then(|s| s.parse().ok())
        .ok_or_else(invalid)?;
    if super::identity(owner).is_none_or(|id| id.start != start || id.zombie)
        || super::identity(std::process::id()).is_none_or(|id| id.parent != owner)
    {
        return Err(invalid());
    }
    let daemon = membership(owner)?;
    let path = Path::new(&args[2]);
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .and_then(|s| s.strip_prefix("run-"))
        .ok_or_else(invalid)?;
    if daemon.file_name().is_none_or(|s| s != "daemon")
        || name.len() != 16
        || !name.bytes().all(|c| c.is_ascii_hexdigit())
        || path.parent()
            != Some(
                daemon
                    .parent()
                    .ok_or_else(invalid)?
                    .join("runner")
                    .as_path(),
            )
    {
        return Err(invalid());
    }
    let group = Group::open(path.to_owned())?;
    file(&group.directory, "cgroup.procs", OFlags::WRONLY)?.write_all_at(b"0", 0)?;
    if membership(std::process::id())? != path {
        return Err(invalid());
    }
    Err(Command::new(&args[3]).args(&args[4..]).exec())
}

#[derive(Debug, Default)]
pub(super) struct Kernel {
    pub(super) managed: Option<Managed>,
    pub(super) error: Option<&'static str>,
}

impl Kernel {
    pub(super) fn ready(&self, requested: MemoryLimits) -> bool {
        self.managed
            .as_ref()
            .is_none_or(|managed| self.error.is_none() && managed.matches(requested))
    }

    pub(super) fn status(&self, requested: MemoryLimits) -> Value {
        match &self.managed {
            Some(managed) => match managed.status(requested) {
                Ok(mut status) => {
                    status["error"] = json!(self.error);
                    status
                }
                Err(_) => {
                    json!({"available":true,"complete":false,"enforced":false,"error":"CGROUP_MEASUREMENT_UNAVAILABLE"})
                }
            },
            None => {
                json!({"available":false,"complete":false,"enforced":false,"error":self.error.unwrap_or("CGROUP_UNAVAILABLE")})
            }
        }
    }
}
