//! Native CLI process ownership shared by version probes and runners.
use super::{AdapterError, ErrorCode, Result};
use rustix::process::{
    Pid, Signal, WaitId, WaitIdOptions, kill_process, kill_process_group, waitid,
};
use std::{
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt};

pub(crate) async fn version_output(binary: &Path, timeout: Duration) -> Result<(PathBuf, String)> {
    let executable = resolve(binary)?;
    let mut command = std::process::Command::new(&executable);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = tokio::process::Command::from(command)
        .kill_on_drop(true)
        .spawn()
        .map_err(spawn_error)?;
    let group = ProcessGroup(
        Pid::from_raw(child.id().ok_or(ErrorCode::RunnerFailed)? as i32)
            .ok_or(ErrorCode::RunnerFailed)?,
        false,
    );
    let stdout = child.stdout.take().ok_or(ErrorCode::RunnerFailed)?;
    let stderr = child.stderr.take().ok_or(ErrorCode::RunnerFailed)?;
    let result = tokio::time::timeout(timeout, async {
        tokio::try_join!(bounded(stdout), bounded(stderr), wait_for_exit(&child))
    })
    .await
    .map_err(|_| ErrorCode::RunnerTimeout);
    drop(group);
    let status = reap(child, false, || {}).wait().await;
    let (stdout, _, ()) = result??;
    if !status.ok_or(ErrorCode::RunnerTimeout)?.success() {
        return Err(ErrorCode::RunnerFailed.into());
    }
    let text = String::from_utf8(stdout).map_err(|_| ErrorCode::AdapterUnsupported)?;
    Ok((executable, text))
}

pub(crate) fn spawn_error(error: std::io::Error) -> AdapterError {
    AdapterError {
        code: ErrorCode::ExecutorUnavailable,
        source_code: error.raw_os_error().map(|code| format!("SPAWN_OS_{code}")),
    }
}

pub(crate) fn resolve(binary: &Path) -> Result<PathBuf> {
    let candidates = if binary.components().count() > 1 || binary.is_absolute() {
        vec![binary.to_owned()]
    } else {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|dir| dir.join(binary))
            .collect()
    };
    candidates
        .into_iter()
        .find(|p| {
            p.metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
        .and_then(|p| p.canonicalize().ok())
        .ok_or_else(|| ErrorCode::ExecutorUnavailable.into())
}

pub(crate) struct ProcessGroup(pub(crate) Pid, pub(crate) bool);
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if self.1 {
            // The unreaped guardian retains its PID and must settle the native tree itself.
            let _ = kill_process(self.0, Signal::TERM);
        } else {
            let _ = kill_process_group(self.0, Signal::KILL);
        }
    }
}

pub(crate) async fn bounded(input: impl AsyncRead + Unpin) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    input
        .take(4097)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| ErrorCode::RunnerFailed)?;
    if bytes.len() > 4096 {
        return Err(ErrorCode::OutputLimit.into());
    }
    Ok(bytes)
}

// Observe exit without releasing the leader PID before signalling its process group.
pub(crate) async fn wait_for_exit(child: &tokio::process::Child) -> Result<()> {
    let pid = Pid::from_raw(child.id().ok_or(ErrorCode::RunnerFailed)? as i32)
        .ok_or(ErrorCode::RunnerFailed)?;
    loop {
        match waitid(
            WaitId::Pid(pid),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        ) {
            Ok(Some(_)) => return Ok(()),
            Ok(None) | Err(rustix::io::Errno::INTR) => {
                tokio::time::sleep(Duration::from_millis(10)).await
            }
            Err(_) => return Err(ErrorCode::RunnerFailed.into()),
        }
    }
}

const REAP_GRACE: Duration = Duration::from_secs(1);

pub(crate) struct Reap {
    receiver: Option<tokio::sync::oneshot::Receiver<ExitStatus>>,
    status: Option<ExitStatus>,
    deadline: tokio::time::Instant,
}
impl Reap {
    pub(crate) fn status(&mut self) -> Option<ExitStatus> {
        if let Some(receiver) = &mut self.receiver {
            match receiver.try_recv() {
                Ok(status) => {
                    self.status = Some(status);
                    self.receiver = None;
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => self.receiver = None,
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
            }
        }
        self.status
    }
    pub(crate) async fn wait(&mut self) -> Option<ExitStatus> {
        if self.status().is_some() {
            return self.status;
        }
        if let Some(receiver) = &mut self.receiver
            && let Ok(Ok(status)) = tokio::time::timeout_at(self.deadline, receiver).await
        {
            self.status = Some(status);
            self.receiver = None;
        }
        self.status
    }
}

pub(crate) fn reap(
    mut child: tokio::process::Child,
    guarded: bool,
    cleanup: impl FnOnce() + Send + 'static,
) -> Reap {
    if !guarded {
        let _ = child.start_kill();
    }
    reap_with(move || child.try_wait(), cleanup)
}

fn reap_with(
    mut observe: impl FnMut() -> std::io::Result<Option<ExitStatus>> + Send + 'static,
    cleanup: impl FnOnce() + Send + 'static,
) -> Reap {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    // This owner outlives both the caller future and its Tokio runtime.
    std::thread::spawn(move || {
        loop {
            if let Ok(Some(status)) = observe() {
                cleanup();
                let _ = sender.send(status);
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    Reap {
        receiver: Some(receiver),
        status: None,
        deadline: tokio::time::Instant::now() + REAP_GRACE,
    }
}

#[cfg(test)]
mod cleanup_tests {
    use super::*;
    use crate::{
        adapter::{BudgetLimits, RunBudget},
        config::deadline::{Deadline, Layer},
    };
    use std::{os::unix::process::ExitStatusExt, sync::Arc};

    struct Release(Option<std::sync::mpsc::Sender<()>>);
    impl Drop for Release {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    #[tokio::test]
    async fn guarded_reap_keeps_live_owner_after_bounded_wait_and_caller_drop() {
        let slots = Arc::new(tokio::sync::Semaphore::new(1));
        let budget = RunBudget::new(
            "origin",
            "fence",
            slots.clone(),
            BudgetLimits::default(),
            Deadline::start(Layer::Workflow, Duration::from_secs(30), None).unwrap(),
        )
        .unwrap();
        let lease = budget.start().unwrap();
        let mut child = tokio::process::Command::new("/bin/cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let pid = child.id().unwrap();
        let (settled, done) = tokio::sync::oneshot::channel();
        let mut cleanup = reap(child, true, move || {
            drop(lease);
            let _ = settled.send(());
        });
        assert!(cleanup.wait().await.is_none());
        drop(cleanup);
        assert_eq!(slots.available_permits(), 0);
        assert!(std::path::Path::new(&format!("/proc/{pid}")).exists());
        drop(input);
        tokio::time::timeout(Duration::from_secs(2), done)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(slots.available_permits(), 1);
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    }

    #[tokio::test]
    async fn pending_reap_is_bounded_and_keeps_lease_until_settled_after_caller_drop() {
        let slots = Arc::new(tokio::sync::Semaphore::new(1));
        let budget = RunBudget::new(
            "origin",
            "fence",
            slots.clone(),
            BudgetLimits::default(),
            Deadline::start(Layer::Workflow, Duration::from_secs(30), None).unwrap(),
        )
        .unwrap();
        let lease = budget.start().unwrap();
        let directory =
            std::env::temp_dir().join(format!("akasha-native-reap-owner-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let cleanup_directory = directory.clone();
        let (release, released) = std::sync::mpsc::channel();
        let release = Release(Some(release));
        let (entered, entry) = tokio::sync::oneshot::channel();
        let mut entered = Some(entered);
        let (settled, done) = tokio::sync::oneshot::channel();
        let mut cleanup = reap_with(
            move || {
                if let Some(entered) = entered.take() {
                    let _ = entered.send(());
                }
                Ok(released.try_recv().ok().map(|()| ExitStatus::from_raw(0)))
            },
            move || {
                std::fs::remove_dir(cleanup_directory).unwrap();
                drop(lease);
                let _ = settled.send(());
            },
        );
        entry.await.unwrap();
        let result = tokio::time::timeout(Duration::from_secs(2), cleanup.wait()).await;
        assert!(
            result.is_ok(),
            "cleanup wait must return before the child can be reaped"
        );
        assert!(result.unwrap().is_none());
        assert!(
            tokio::time::timeout(Duration::from_millis(100), cleanup.wait())
                .await
                .unwrap()
                .is_none(),
            "a second wait must not renew cleanup grace"
        );
        assert!(directory.exists());
        assert_eq!(slots.available_permits(), 0);
        drop(cleanup);
        assert_eq!(slots.available_permits(), 0);
        drop(release);
        tokio::time::timeout(Duration::from_secs(2), done)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(slots.available_permits(), 1);
        assert!(!directory.exists());
    }
}
