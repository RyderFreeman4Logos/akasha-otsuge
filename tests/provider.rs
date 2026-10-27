use akasha_otsuge::{
    config::{
        Paths,
        deadline::{Deadline, Layer},
        resolve,
    },
    provider::{Client, Plan},
};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};

struct HeldEndpoint {
    url: String,
    requests: mpsc::Receiver<oneshot::Sender<()>>,
    worker: tokio::task::JoinHandle<()>,
}
impl HeldEndpoint {
    async fn new() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let (send, requests) = mpsc::channel(4);
        let worker = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    header.push(socket.read_u8().await.unwrap());
                    assert!(header.len() < 16384);
                }
                let header = String::from_utf8(header).unwrap();
                let len = header
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                assert!(len < 65536);
                let mut body = vec![0; len];
                socket.read_exact(&mut body).await.unwrap();
                let (release, wait) = oneshot::channel();
                if send.send(release).await.is_err() {
                    break;
                }
                if wait.await.is_err() {
                    continue;
                }
                let response = json!({"choices":[{"index":0,"message":{"role":"assistant","content":"done"},"finish_reason":"stop"}]}).to_string();
                let bytes = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                    response.len()
                );
                let _ = socket.write_all(bytes.as_bytes()).await;
            }
        });
        Self {
            url,
            requests,
            worker,
        }
    }
}
impl Drop for HeldEndpoint {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

fn plan(count: usize) -> Plan {
    serde_json::from_value(json!({"calls":(0..count).map(|_|json!({"profile":"text","input":{"kind":"text","prompt":"hello","max_output_tokens":32}})).collect::<Vec<_>>()})).unwrap()
}

async fn virtual_calls(count: usize, delay: u64) -> Vec<Value> {
    let mut endpoint = HeldEndpoint::new().await;
    let config = resolve(
        &format!(
            "[models.text]\nkind='text'\nendpoint={:?}\nmodel='fixture'\nlocal=true",
            endpoint.url
        ),
        None,
        "",
        &Paths {
            data_dir: "/unused".into(),
            socket_path: "/unused/socket".into(),
        },
    )
    .unwrap();
    let plan = plan(count);
    let task = tokio::spawn(async move {
        let client = Client::new().unwrap();
        let workflow =
            Deadline::start(Layer::Workflow, plan.validate(&config).unwrap(), None).unwrap();
        let mut outcomes = Vec::new();
        for call in &plan.calls {
            outcomes.push(
                serde_json::to_value(
                    client
                        .call(&config, call, &workflow, std::future::pending())
                        .await,
                )
                .unwrap(),
            );
        }
        outcomes
    });
    for _ in 0..count {
        let release = tokio::time::timeout(Duration::from_secs(5), endpoint.requests.recv())
            .await
            .unwrap()
            .unwrap();
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(delay)).await;
        tokio::task::yield_now().await;
        tokio::time::resume();
        let _ = release.send(());
    }
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn provider_1699_success_1700_timeout_and_serial_stage_budgets() {
    let success = virtual_calls(3, 1699).await;
    for call in &success {
        assert!(call["error"].is_null(), "{call}");
        assert_eq!(call["output"]["text"], "done");
        assert_eq!(call["trace"]["provider"]["configured_ms"], 1_700_000);
        assert!(call["trace"]["provider"]["elapsed_ms"].as_u64().unwrap() >= 1_699_000);
        assert!(
            call["trace"]["queue"]["elapsed_ms"].as_u64().unwrap() < 1000,
            "queue clock included provider execution: {call}"
        );
        assert!(call["trace"]["queue"]["triggered_layer"].is_null());
    }
    assert!(
        success[2]["trace"]["workflow"]["elapsed_ms"]
            .as_u64()
            .unwrap()
            >= 5_097_000
    );
    let timeout = virtual_calls(1, 1700).await;
    assert_eq!(timeout[0]["error"], "PROVIDER_TIMEOUT");
    assert_eq!(
        timeout[0]["trace"]["provider"]["triggered_layer"],
        "provider"
    );
}
