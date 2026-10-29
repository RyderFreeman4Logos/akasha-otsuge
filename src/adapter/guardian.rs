use super::{ErrorCode, Result};
use rustix::process::{
    Pid, PidfdFlags, Signal, WaitId, WaitIdOptions, getpgrp, getpid, pidfd_open, pidfd_send_signal,
    set_child_subreaper, waitid,
};
use std::{
    ffi::OsString,
    os::unix::{fs::MetadataExt, process::ExitStatusExt},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

struct Identity {
    parent: u32,
    start: u64,
    zombie: bool,
}
fn identity(pid: u32) -> Option<Identity> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, tail) = stat.rsplit_once(") ")?;
    let fields: Vec<_> = tail.split_whitespace().collect();
    Some(Identity {
        parent: fields.get(1)?.parse().ok()?,
        start: fields.get(19)?.parse().ok()?,
        zombie: *fields.first()? == "Z",
    })
}

pub(super) fn command(guard: &Path, binary: &Path, directory: &Path) -> Result<Command> {
    let owner = std::process::id();
    let start = identity(owner).ok_or(ErrorCode::SourceUnavailable)?.start;
    let mut command = Command::new(guard);
    command.args([
        OsString::from("__native-runner"),
        owner.to_string().into(),
        start.to_string().into(),
        directory.as_os_str().to_owned(),
        binary.as_os_str().to_owned(),
    ]);
    Ok(command)
}

fn cleanup() -> Result<()> {
    let self_pid = std::process::id();
    let end = Instant::now() + Duration::from_secs(1);
    loop {
        // This single-threaded subreaper owns every PID in its children file.
        let children = std::fs::read_to_string(format!("/proc/self/task/{self_pid}/children"))
            .map_err(|_| ErrorCode::RunnerFailed)?;
        for pid in children.split_whitespace() {
            let pid = pid
                .parse::<i32>()
                .ok()
                .and_then(Pid::from_raw)
                .ok_or(ErrorCode::RunnerFailed)?;
            if let Ok(fd) = pidfd_open(pid, PidfdFlags::empty())
                && identity(pid.as_raw_nonzero().get() as u32).is_some_and(|p| p.parent == self_pid)
            {
                let _ = pidfd_send_signal(fd, Signal::KILL);
            }
        }
        loop {
            match waitid(WaitId::All, WaitIdOptions::EXITED | WaitIdOptions::NOHANG) {
                Ok(Some(_)) => continue,
                Err(rustix::io::Errno::CHILD) => return Ok(()),
                Ok(None) | Err(rustix::io::Errno::INTR) => break,
                Err(_) => return Err(ErrorCode::RunnerFailed.into()),
            }
        }
        if Instant::now() >= end {
            return Err(ErrorCode::RunnerFailed.into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Internal stdio-transparent owner for daemon-launched native workers.
#[doc(hidden)]
pub fn run_guard(args: &[OsString]) -> Result<u8> {
    if args.len() < 4 || getpgrp() != getpid() {
        return Err(ErrorCode::InvalidArgument.into());
    }
    let owner = args[0]
        .to_str()
        .and_then(|s| s.parse::<u32>().ok())
        .ok_or(ErrorCode::InvalidArgument)?;
    let start = args[1]
        .to_str()
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or(ErrorCode::InvalidArgument)?;
    let directory = Path::new(&args[2]);
    let name = directory
        .file_name()
        .and_then(|s| s.to_str())
        .and_then(|s| s.strip_prefix("akasha-run-"))
        .ok_or(ErrorCode::InvalidArgument)?;
    let meta = std::fs::symlink_metadata(directory).map_err(|_| ErrorCode::SourceUnavailable)?;
    if name.len() != 64
        || !name.bytes().all(|b| b.is_ascii_hexdigit())
        || !meta.is_dir()
        || meta.uid() != rustix::process::geteuid().as_raw()
        || meta.mode() & 0o777 != 0o700
        || std::env::current_dir().map_err(|_| ErrorCode::SourceUnavailable)?
            != directory
                .canonicalize()
                .map_err(|_| ErrorCode::SourceUnavailable)?
    {
        return Err(ErrorCode::ReadDenied.into());
    }
    let alive = || identity(owner).is_some_and(|p| p.start == start && !p.zombie);
    if !alive() {
        std::fs::remove_dir_all(directory).map_err(|_| ErrorCode::SourceUnavailable)?;
        return Err(ErrorCode::Cancelled.into());
    }
    if identity(std::process::id()).is_none_or(|p| p.parent != owner) {
        return Err(ErrorCode::ReadDenied.into());
    }
    set_child_subreaper(Some(getpid())).map_err(|_| ErrorCode::RunnerFailed)?;
    let mut child = Command::new(&args[3])
        .args(&args[4..])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(super::native::spawn_error)?;
    let code = loop {
        if !alive() {
            break 125;
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                break status.code().unwrap_or(128 + status.signal().unwrap_or(0)) as u8;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => {
                cleanup()?;
                return Err(ErrorCode::RunnerFailed.into());
            }
        }
    };
    cleanup()?;
    if !alive() {
        std::fs::remove_dir_all(directory).map_err(|_| ErrorCode::SourceUnavailable)?;
    }
    Ok(code)
}
