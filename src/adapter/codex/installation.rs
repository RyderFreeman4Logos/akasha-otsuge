//! A single opt-in launcher; native user/project settings are untouched.
use super::hooks::quote;
use crate::{
    ipc::{Error, Result},
    storage::hash,
};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

const MARKER: &str = "# akasha-otsuge-codex-v1 ";

fn destination(path: &Path) -> Result<PathBuf> {
    let path = std::path::absolute(path)?;
    Ok(path
        .parent()
        .ok_or(Error("UNSAFE_PATH"))?
        .canonicalize()?
        .join(path.file_name().ok_or(Error("UNSAFE_PATH"))?))
}

fn read(path: &Path) -> Result<Vec<u8>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.uid() != rustix::process::geteuid().as_raw()
        || meta.nlink() != 1
        || meta.mode() & 0o777 != 0o700
        || meta.len() > 65536
    {
        return Err(Error("ADAPTER_INSTALL_CONFLICT"));
    }
    let mut bytes = Vec::new();
    file.take(65537).read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err(Error("ADAPTER_INSTALL_CONFLICT"));
    }
    Ok(bytes)
}

pub async fn install(binary: &Path, launcher: &Path, state: &Path, socket: &Path) -> Result<Value> {
    let report = super::probe(binary).await?;
    if report.version.as_deref() != Some("0.164.0") {
        return Err(Error("ADAPTER_UNSUPPORTED"));
    }
    let binary = report.executable.ok_or(Error("EXECUTOR_UNAVAILABLE"))?;
    let launcher = destination(launcher)?;
    let state = destination(state)?;
    if !socket.is_absolute() {
        return Err(Error("UNSAFE_PATH"));
    }
    let bridge = std::env::current_exe()?.canonicalize()?;
    let words = [&bridge, socket, &binary, &state]
        .into_iter()
        .map(|p| p.to_str().map(quote).ok_or(Error("UNSAFE_PATH")))
        .collect::<Result<Vec<_>>>()?;
    let hook = format!(
        "{} --socket {} adapter codex-hook --binary {} --state-dir {}",
        words[0], words[1], words[2], words[3]
    );
    let mut body = format!("exec {}", words[2]);
    for event in [
        "SessionStart",
        "PreToolUse",
        "PostToolUse",
        "PostToolUseFailure",
        "Stop",
    ] {
        let matcher = if matches!(event, "PreToolUse" | "PostToolUse" | "PostToolUseFailure") {
            "matcher=\"^Bash$\","
        } else {
            ""
        };
        let config = format!(
            "hooks.{event}=[{{{matcher}hooks=[{{type=\"command\",command={},timeout=30}}]}}]",
            serde_json::to_string(&hook)?
        );
        body.push_str(&format!(" -c {}", quote(&config)));
    }
    body.push_str(" \"$@\"\n");
    let bytes = format!("#!/bin/sh\n{MARKER}{}\n{body}", hash(body.as_bytes())).into_bytes();
    if fs::symlink_metadata(&launcher).is_ok() {
        if read(&launcher)? != bytes {
            return Err(Error("ADAPTER_INSTALL_CONFLICT"));
        }
    } else {
        let mut nonce = [0u8; 16];
        File::open("/dev/urandom")?.read_exact(&mut nonce)?;
        let temporary = launcher.with_file_name(format!(".akasha-codex-{}", hash(&nonce)));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o700)
            .open(&temporary)?;
        let result = (|| {
            file.write_all(&bytes)?;
            file.sync_all()?;
            rustix::fs::renameat_with(
                rustix::fs::CWD,
                &temporary,
                rustix::fs::CWD,
                &launcher,
                rustix::fs::RenameFlags::NOREPLACE,
            )
            .map_err(std::io::Error::from)?;
            File::open(launcher.parent().ok_or(Error("UNSAFE_PATH"))?)?.sync_all()?;
            Ok::<_, Error>(())
        })();
        drop(file);
        if temporary.try_exists()? {
            fs::remove_file(temporary)?;
        }
        result?;
    }
    Ok(
        json!({"launcher":launcher,"state_dir":state,"version":"0.164.0","hook_trust":"native_review_required","settings_modified":false}),
    )
}

pub fn uninstall(launcher: &Path) -> Result<Value> {
    let path = destination(launcher)?;
    let bytes = read(&path)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| Error("ADAPTER_INSTALL_CONFLICT"))?;
    let mut lines = text.splitn(3, '\n');
    let shebang = lines.next();
    let marker = lines.next();
    let body = lines.next();
    if shebang != Some("#!/bin/sh")
        || body.is_none()
        || marker.and_then(|s| s.strip_prefix(MARKER))
            != body.map(|s| hash(s.as_bytes())).as_deref()
    {
        return Err(Error("ADAPTER_INSTALL_CONFLICT"));
    }
    fs::remove_file(&path)?;
    Ok(json!({"launcher":path,"state":"removed","settings_modified":false}))
}
