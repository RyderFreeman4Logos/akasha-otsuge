use crate::adapter::{ContextRequest, ErrorCode, MAX_FRAME_BYTES, Result};
use serde_json::{Value, json};
use std::io::Read;
use tokio::net::UnixStream;

/// Private native-plugin transport; model input contains only a context operation.
pub async fn read_bridge() -> Result<Value> {
    let mut input = Vec::new();
    std::io::stdin()
        .take(MAX_FRAME_BYTES as u64 + 1)
        .read_to_end(&mut input)
        .map_err(|_| ErrorCode::ProtocolInvalid)?;
    if input.len() > MAX_FRAME_BYTES {
        return Err(ErrorCode::OutputLimit.into());
    }
    let request: ContextRequest =
        serde_json::from_slice(&input).map_err(|_| ErrorCode::ProtocolInvalid)?;
    let socket = std::env::var_os("AKASHA_RUN_SOCKET").ok_or(ErrorCode::BindingUnavailable)?;
    let token = std::env::var("AKASHA_RUN_TOKEN").map_err(|_| ErrorCode::BindingUnavailable)?;
    if token.is_empty() || token.len() > 256 {
        return Err(ErrorCode::BindingUnavailable.into());
    }
    let deadline = crate::adapter::native::bridge_deadline()?;
    let handshake = deadline.min(tokio::time::Instant::now() + crate::ipc::TRANSPORT_TIMEOUT);
    let mut stream = tokio::time::timeout_at(handshake, async {
        let mut stream = UnixStream::connect(socket)
            .await
            .map_err(|_| ErrorCode::SourceUnavailable)?;
        if !stream
            .peer_cred()
            .is_ok_and(|peer| peer.uid() == rustix::process::geteuid().as_raw())
        {
            return Err(ErrorCode::ReadDenied.into());
        }
        crate::ipc::write(&mut stream, &json!({"token":token,"request":request}))
            .await
            .map_err(|_| ErrorCode::SourceUnavailable)?;
        Ok::<_, crate::adapter::AdapterError>(stream)
    })
    .await
    .map_err(|_| ErrorCode::SourceUnavailable)??;
    tokio::time::timeout_at(
        deadline,
        crate::ipc::read_bounded(&mut stream, MAX_FRAME_BYTES),
    )
    .await
    .map_err(|_| ErrorCode::RunnerTimeout)?
    .map_err(|_| ErrorCode::ProtocolInvalid.into())
}
