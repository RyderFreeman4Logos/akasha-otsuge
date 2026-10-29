use super::*;
use akasha_otsuge::ipc;
use serde_json::{Value, json};
use std::{os::fd::AsRawFd, os::linux::net::SocketAddrExt, process::Stdio, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn send(stream: &mut tokio::net::UnixStream, value: &Value) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(value).unwrap();
    stream.write_u32(bytes.len() as u32).await?;
    stream.write_all(&bytes).await
}

async fn receive(stream: &mut tokio::net::UnixStream) -> Value {
    let size = stream.read_u32().await.unwrap();
    assert!(size > 0 && size <= 16384);
    let mut bytes = vec![0; size as usize];
    stream.read_exact(&mut bytes).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn private_bridges_wait_for_research_beyond_the_transport_timeout() {
    let (claude, pi) = tokio::join!(bridge(false, 30000, 11000), bridge(true, 30000, 11000));
    for output in [claude, pi] {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(reply["data"]["answer"], "slow research");
    }
}

#[tokio::test]
async fn private_bridges_stop_at_the_original_deadline() {
    let (claude, pi) = tokio::join!(bridge(false, 2000, 2500), bridge(true, 2000, 2500));
    for output in [claude, pi] {
        assert!(!output.status.success());
        let reply: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(reply["error"]["code"], "RUNNER_TIMEOUT");
    }
}

#[tokio::test]
async fn private_bridges_reject_missing_malformed_and_expired_deadlines() {
    for pi in [false, true] {
        for (deadline, code) in [
            (None, "CONFIG_INVALID"),
            (Some("invalid"), "CONFIG_INVALID"),
            (Some("18446744073709551616"), "CONFIG_INVALID"),
            (Some("1"), "RUNNER_TIMEOUT"),
        ] {
            let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_akasha-otsuge"));
            command
                .args(["adapter", if pi { "pi-context" } else { "claude-read" }])
                .env_remove("AKASHA_RUN_DEADLINE_MS")
                .env("AKASHA_RUN_SOCKET", "/nonexistent/akasha-deadline")
                .env("AKASHA_RUN_TOKEN", "b".repeat(64))
                .env("AKASHA_PI_RUN_SOCKET", format!("akasha-{}", "a".repeat(64)))
                .env("AKASHA_PI_RUN_TOKEN", "b".repeat(64))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            if let Some(deadline) = deadline {
                command.env("AKASHA_RUN_DEADLINE_MS", deadline);
            }
            let mut child = command.spawn().unwrap();
            let request = if pi {
                json!({"op":"context","request":{"operation":"describe"}})
            } else {
                json!({"operation":"describe"})
            };
            child
                .stdin
                .take()
                .unwrap()
                .write_all(&serde_json::to_vec(&request).unwrap())
                .await
                .unwrap();
            let output = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output())
                .await
                .unwrap()
                .unwrap();
            assert!(!output.status.success());
            let reply: Value = serde_json::from_slice(&output.stderr).unwrap();
            assert_eq!(reply["error"]["code"], code);
        }
    }
}

async fn bridge(pi: bool, budget_ms: u64, delay_ms: u64) -> std::process::Output {
    let fixture = Fixture::new();
    let directory = std::fs::File::open(&fixture.0).unwrap();
    let socket = format!(
        "/proc/{}/fd/{}/s",
        std::process::id(),
        directory.as_raw_fd()
    );
    let name = format!(
        "akasha-{:032x}{:032x}",
        std::process::id(),
        directory.as_raw_fd()
    );
    let raw = if pi {
        std::os::unix::net::UnixListener::bind_addr(
            &std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes()).unwrap(),
        )
        .unwrap()
    } else {
        std::os::unix::net::UnixListener::bind(&socket).unwrap()
    };
    raw.set_nonblocking(true).unwrap();
    let listener = tokio::net::UnixListener::from_std(raw).unwrap();
    let token = "b".repeat(64);
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_akasha-otsuge"));
    child
        .args(["adapter", if pi { "pi-context" } else { "claude-read" }])
        .env(
            "AKASHA_RUN_DEADLINE_MS",
            (ipc::now_ms().unwrap() + budget_ms).to_string(),
        )
        .env(
            if pi {
                "AKASHA_PI_RUN_TOKEN"
            } else {
                "AKASHA_RUN_TOKEN"
            },
            &token,
        )
        .env(
            if pi {
                "AKASHA_PI_RUN_SOCKET"
            } else {
                "AKASHA_RUN_SOCKET"
            },
            if pi { &name } else { &socket },
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = child.spawn().unwrap();
    let request = if pi {
        json!({"op":"context","request":{"operation":"delegate","hint":"child"}})
    } else {
        json!({"operation":"delegate","hint":"child"})
    };
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&request).unwrap())
        .await
        .unwrap();
    let serve = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        if pi {
            send(
                &mut stream,
                &json!({"protocol_version":1,"max_message_bytes":ipc::MAX_MESSAGE}),
            )
            .await
            .unwrap();
        }
        let request = receive(&mut stream).await;
        assert_eq!(
            if pi {
                &request["command"]["token"]
            } else {
                &request["token"]
            },
            &token
        );
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        let reply = if pi {
            json!({"schema_version":1,"sequence":1,"request_id":request["request_id"],"data":{"answer":"slow research"}})
        } else {
            json!({"data":{"answer":"slow research"}})
        };
        let _ = send(&mut stream, &reply).await;
    };
    let (output, ()) = tokio::time::timeout(Duration::from_secs(35), async {
        tokio::join!(child.wait_with_output(), serve)
    })
    .await
    .unwrap();
    output.unwrap()
}
