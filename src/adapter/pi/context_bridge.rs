//! Short-lived extension client for the owned Runner's scoped context broker.
use crate::ipc::{self, Error, Result};
use serde_json::{Value, json};

pub async fn call(event: Value) -> Result<Value> {
    let name = std::env::var("AKASHA_PI_RUN_SOCKET").map_err(|_| Error("CONFIG_INVALID"))?;
    let token = std::env::var("AKASHA_PI_RUN_TOKEN").map_err(|_| Error("CONFIG_INVALID"))?;
    let hex = |s: &str| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit());
    if !name.strip_prefix("akasha-").is_some_and(hex) || !hex(&token) {
        return Err(Error("CONFIG_INVALID"));
    }
    let id = format!("pi-{}-{}", std::process::id(), ipc::now_ms()?);
    let request = json!({"protocol_version":1,"request_id":id,"command":{"op":"pi_context","token":token,"event":event}});
    if serde_json::to_vec(&request)?.len() > 16384 {
        return Err(Error("MESSAGE_TOO_LARGE"));
    }
    let deadline = crate::adapter::native::bridge_deadline()?;
    let connect_deadline = deadline.min(tokio::time::Instant::now() + ipc::TRANSPORT_TIMEOUT);
    let mut stream = tokio::time::timeout_at(connect_deadline, async {
        // Nonblocking connect is completed by Tokio; no unbounded synchronous connect.
        let fd = rustix::net::socket_with(
            rustix::net::AddressFamily::UNIX,
            rustix::net::SocketType::STREAM,
            rustix::net::SocketFlags::NONBLOCK | rustix::net::SocketFlags::CLOEXEC,
            None,
        )
        .map_err(std::io::Error::from)?;
        let raw = std::os::unix::net::UnixStream::from(fd);
        let native_addr = rustix::net::SocketAddrUnix::new_abstract_name(name.as_bytes())
            .map_err(std::io::Error::from)?;
        match rustix::net::connect(&raw, &native_addr) {
            Ok(()) => {}
            Err(rustix::io::Errno::INPROGRESS) => {}
            Err(_) => return Err(Error("DAEMON_UNAVAILABLE")),
        }
        let stream = tokio::net::UnixStream::from_std(raw)?;
        stream.writable().await?;
        if stream.take_error()?.is_some() {
            return Err(Error("DAEMON_UNAVAILABLE"));
        }
        if stream.peer_cred()?.uid() != rustix::process::geteuid().as_raw() {
            return Err(Error("PEER_DENIED"));
        }
        Ok::<_, Error>(stream)
    })
    .await
    .map_err(|_| Error("IPC_TIMEOUT"))??;
    // A preceding delegated callback can delay the broker's greeting.
    let reply: Value = tokio::time::timeout_at(deadline, async {
        let greeting: Value = ipc::read(&mut stream).await?;
        if greeting["protocol_version"] != 1 || greeting["max_message_bytes"] != ipc::MAX_MESSAGE {
            return Err(Error("PROTOCOL_VERSION"));
        }
        ipc::write(&mut stream, &request).await?;
        ipc::read(&mut stream).await
    })
    .await
    .map_err(|_| Error("RUNNER_TIMEOUT"))??;
    if reply["schema_version"] != 1 || reply["sequence"] != 1 || reply["request_id"] != id {
        return Err(Error("PROTOCOL_INVALID"));
    }
    Ok(reply)
}
