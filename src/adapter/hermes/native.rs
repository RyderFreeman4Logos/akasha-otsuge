use super::HermesCall;
use crate::adapter::{
    ErrorCode, Result,
    native::{self, OwnedChild},
};
use std::{
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::io::AsyncReadExt;

/// Private host transport. These fields are never included in the model tool schema.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeEvent {
    Bind {
        binary: PathBuf,
        home: PathBuf,
        call: HermesCall,
    },
    Consult {
        binary: PathBuf,
        home: PathBuf,
        call: HermesCall,
        args: crate::adapter::ConsultArgs,
    },
    Sync {
        binary: PathBuf,
        home: PathBuf,
        session_id: String,
    },
}

pub(crate) async fn export_session(binary: &Path, home: &Path, session: &str) -> Result<Vec<u8>> {
    if !super::super::valid_id(session) || !home.is_absolute() {
        return Err(ErrorCode::InvalidArgument.into());
    }
    let executable = native::resolve(binary)?;
    let mut cmd = native::resource_command(&executable)?;
    cmd.args([
        "sessions",
        "export",
        "-",
        "--session-id",
        session,
        "--format",
        "jsonl",
    ])
    .env("HERMES_HOME", home)
    .env("HERMES_DISABLE_LAZY_INSTALLS", "1")
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .process_group(0);
    let child = tokio::process::Command::from(cmd)
        .kill_on_drop(true)
        .spawn()
        .map_err(native::spawn_error)?;
    let mut owner = OwnedChild::new(child)?;
    let stdout = owner.child().stdout.take().ok_or(ErrorCode::RunnerFailed)?;
    let stderr = owner.child().stderr.take().ok_or(ErrorCode::RunnerFailed)?;
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        let output = async {
            let mut bytes = Vec::new();
            stdout
                .take(8 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| ErrorCode::SourceUnavailable)?;
            if bytes.len() > 8 * 1024 * 1024 {
                return Err(ErrorCode::OutputLimit.into());
            }
            Ok::<_, crate::adapter::AdapterError>(bytes)
        };
        tokio::try_join!(
            output,
            native::bounded(stderr),
            native::wait_for_exit(owner.child())
        )
    })
    .await
    .map_err(|_| ErrorCode::RunnerTimeout);
    let status = owner.reap().wait().await;
    let (bytes, _, ()) = result??;
    if !status.ok_or(ErrorCode::RunnerTimeout)?.success() {
        return Err(ErrorCode::SourceUnavailable.into());
    }
    Ok(bytes)
}
