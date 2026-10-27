use super::*;
use std::net::TcpListener;
use std::sync::{Arc, Mutex, atomic::AtomicBool};

struct Endpoint {
    address: String,
    requests: Arc<Mutex<Vec<(String, Value)>>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
#[derive(Clone, Copy)]
enum Mode {
    Normal,
    Malformed,
    Disconnect,
    Slow,
    RateLimited,
}
impl Endpoint {
    fn new() -> Self {
        Self::with_mode(Mode::Normal)
    }
    fn with_mode(mode: Mode) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let saved = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = std::thread::spawn(move || {
            while !stopping.load(Ordering::Acquire) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut bytes = Vec::new();
                let (head, len) = loop {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    bytes.push(byte[0]);
                    assert!(bytes.len() <= 16384);
                    if bytes.ends_with(b"\r\n\r\n") {
                        let head = String::from_utf8(bytes.clone()).unwrap();
                        let len = head
                            .lines()
                            .find_map(|s| {
                                let (k, v) = s.split_once(':')?;
                                k.eq_ignore_ascii_case("content-length")
                                    .then(|| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap();
                        break (head, len);
                    }
                };
                assert!(len <= 65536);
                bytes.resize(len, 0);
                stream.read_exact(&mut bytes).unwrap();
                let path = head
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap();
                let body: Value = serde_json::from_slice(&bytes).unwrap();
                saved.lock().unwrap().push((path.into(), body));
                if matches!(mode, Mode::Disconnect) {
                    continue;
                }
                if matches!(mode, Mode::Slow) {
                    std::thread::sleep(Duration::from_millis(400));
                }
                let response = if matches!(mode, Mode::Malformed) {
                    json!({"choices":[],"data":[],"results":[]})
                } else { match path {
                    "/v1/chat/completions" => json!({"choices":[{"index":0,"message":{"role":"assistant","content":"hello"},"finish_reason":"stop"}]}),
                    "/v1/embeddings" => json!({"data":[{"index":1,"embedding":[0.0,1.0]},{"index":0,"embedding":[1.0,0.0]}]}),
                    "/v1/rerank" => json!({"results":[{"index":1,"relevance_score":0.75},{"index":0,"relevance_score":0.25}]}),
                    _ => panic!("unexpected path {path}"),
                }}.to_string();
                let status = if matches!(mode, Mode::RateLimited) {
                    "429 Too Many Requests"
                } else {
                    "200 OK"
                };
                let sent = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.len(),
                    response
                );
                if !matches!(mode, Mode::Slow) {
                    sent.unwrap();
                }
            }
        });
        Self {
            address,
            requests,
            stop,
            worker: Some(worker),
        }
    }
}

fn terminal(daemon: &Daemon, id: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let job = daemon.cli(&["job", "show", id])["data"].clone();
        if matches!(
            job["state"].as_str(),
            Some("succeeded" | "failed" | "cancelled")
        ) {
            return job;
        }
        assert!(Instant::now() < deadline, "{job}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn text_config(endpoint: &Endpoint) -> String {
    format!(
        "[runtime]\nmax_concurrent_jobs=1\n[models.text]\nkind='text'\nendpoint={:?}\nmodel='fixture'\nlocal=true\ntimeout='3s'",
        endpoint.address
    )
}
fn submit_text(daemon: &Daemon, id: &str) -> Value {
    let path = daemon.root.join("plan.json");
    std::fs::write(&path,json!({"calls":[{"profile":"text","input":{"kind":"text","prompt":"hello","max_output_tokens":32}}]}).to_string()).unwrap();
    daemon.cli(&[
        "model",
        "run",
        path.to_str().unwrap(),
        "--idempotency-key",
        id,
    ])["data"]
        .clone()
}
fn await_request(endpoint: &Endpoint) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while endpoint.requests.lock().unwrap().is_empty() {
        assert!(Instant::now() < deadline, "provider request not sent");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn model_cancel_shutdown_and_crash_never_replay_ambiguous_calls() {
    for mode in ["cancel", "shutdown", "crash"] {
        let endpoint = Endpoint::with_mode(Mode::Slow);
        let mut daemon = Daemon::new();
        daemon.configure(&text_config(&endpoint));
        daemon.start();
        submit_text(&daemon, "owned");
        await_request(&endpoint);
        if mode == "cancel" {
            daemon.cli(&["job", "cancel", "owned"]);
        } else {
            if mode == "crash" {
                use std::os::unix::process::ExitStatusExt;
                let mut child = daemon.child.take().unwrap();
                child.kill().unwrap();
                assert_eq!(child.wait().unwrap().signal(), Some(9));
            } else {
                daemon.cli(&["daemon", "stop"]);
                daemon.wait();
            }
            daemon.start();
        }
        let job = terminal(&daemon, "owned");
        assert_eq!(
            job["error"],
            if mode == "cancel" {
                "CANCELLED"
            } else {
                "PROVIDER_INTERRUPTED"
            },
            "{mode}: {job}"
        );
        assert_eq!(
            job["state"],
            if mode == "cancel" {
                "cancelled"
            } else {
                "failed"
            }
        );
        assert_eq!(submit_text(&daemon, "owned")["state"], job["state"]);
        assert_eq!(endpoint.requests.lock().unwrap().len(), 1);
    }
}

#[test]
fn queued_models_keep_endpoint_and_budget_across_reload() {
    let old = Endpoint::with_mode(Mode::Slow);
    let new = Endpoint::new();
    let mut daemon = Daemon::new();
    daemon.configure(&text_config(&old));
    daemon.start();
    submit_text(&daemon, "first");
    await_request(&old);
    let queued = submit_text(&daemon, "queued");
    assert_eq!(queued["state"], "queued");
    daemon.configure(&text_config(&new).replace("timeout='3s'", "timeout='5s'"));
    assert_eq!(
        daemon.cli(&["config", "reload"])["data"]["status"],
        "applied"
    );
    let latest = submit_text(&daemon, "new");
    assert_eq!(latest["config_generation"], 2);
    assert_eq!(terminal(&daemon, "first")["state"], "succeeded");
    let old_result = terminal(&daemon, "queued");
    assert_eq!(old_result["config_generation"], 1);
    assert_eq!(
        old_result["execution_budget_ms"],
        queued["execution_budget_ms"]
    );
    assert_eq!(
        old_result["result"]["calls"][0]["trace"]["provider"]["configured_ms"],
        3000
    );
    assert_eq!(
        terminal(&daemon, "new")["result"]["calls"][0]["trace"]["provider"]["configured_ms"],
        5000
    );
    assert_eq!(old.requests.lock().unwrap().len(), 2);
    assert_eq!(new.requests.lock().unwrap().len(), 1);
}

#[test]
fn all_model_kinds_classify_bad_output_disconnect_timeout_and_rate_limit() {
    for (mode, expected) in [
        (Mode::Malformed, "PROVIDER_OUTPUT_INVALID"),
        (Mode::Disconnect, "PROVIDER_TRANSPORT"),
        (Mode::Slow, "PROVIDER_TIMEOUT"),
        (Mode::RateLimited, "PROVIDER_RATE_LIMITED"),
    ] {
        for kind in ["text", "embedding", "reranker"] {
            let endpoint = Endpoint::with_mode(mode);
            let mut daemon = Daemon::new();
            daemon.configure(&format!("[models.call]\nkind={kind:?}\nendpoint={:?}\nmodel='fixture'\nlocal=true\ndimension=2\ntimeout='200ms'", endpoint.address));
            daemon.start();
            let input = match kind {
                "text" => json!({"kind":kind,"prompt":"hello","max_output_tokens":32}),
                "embedding" => json!({"kind":kind,"texts":["one","two"]}),
                _ => json!({"kind":kind,"query":"query","documents":["one","two"]}),
            };
            let path = daemon.root.join("plan.json");
            std::fs::write(
                &path,
                json!({"calls":[{"profile":"call","input":input}]}).to_string(),
            )
            .unwrap();
            daemon.cli(&[
                "model",
                "run",
                path.to_str().unwrap(),
                "--idempotency-key",
                "failure",
            ]);
            let job = terminal(&daemon, "failure");
            assert_eq!(job["state"], "failed", "{kind}: {job}");
            assert_eq!(job["error"], expected, "{kind}: {job}");
            assert_eq!(endpoint.requests.lock().unwrap().len(), 1);
            assert_eq!(
                job["result"]["calls"][0]["trace"]["provider"]["configured_ms"],
                200
            );
        }
    }
}
impl Drop for Endpoint {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}

#[test]
fn independent_model_endpoints_run_as_durable_jobs() {
    let text = Endpoint::new();
    let embedding = Endpoint::new();
    let reranker = Endpoint::new();
    let mut daemon = Daemon::new();
    daemon.configure(&format!("[models.text]\nkind='text'\nendpoint={:?}\nmodel='fixture-text'\nlocal=true\n[models.embedding]\nkind='embedding'\nendpoint={:?}\nmodel='fixture-vector'\nlocal=true\ndimension=2\n[models.reranker]\nkind='reranker'\nendpoint={:?}\nmodel='fixture-rerank'\nlocal=true", text.address, embedding.address, reranker.address));
    daemon.start();
    let plan = json!({"calls":[
        {"profile":"text","input":{"kind":"text","prompt":"hello","max_output_tokens":32}},
        {"profile":"embedding","input":{"kind":"embedding","texts":["one","two"]}},
        {"profile":"reranker","input":{"kind":"reranker","query":"query","documents":["one","two"]}}
    ]});
    let path = daemon.root.join("plan.json");
    std::fs::write(&path, plan.to_string()).unwrap();
    let args = [
        "model",
        "run",
        path.to_str().unwrap(),
        "--idempotency-key",
        "models",
    ];
    let accepted = daemon.cli(&args)["data"].clone();
    assert_eq!(accepted["id"], "models");
    assert_eq!(accepted["config_generation"], 1);
    let deadline = Instant::now() + Duration::from_secs(15);
    let completed = loop {
        let job = daemon.cli(&["job", "show", "models"])["data"].clone();
        if matches!(
            job["state"].as_str(),
            Some("succeeded" | "failed" | "cancelled")
        ) {
            break job;
        }
        assert!(Instant::now() < deadline, "{job}");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(completed["state"], "succeeded", "{completed}");
    let calls = completed["result"]["calls"].as_array().unwrap();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0]["output"]["text"], "hello");
    assert_eq!(
        calls[1]["output"]["vectors"],
        json!([[1.0, 0.0], [0.0, 1.0]])
    );
    assert_eq!(calls[2]["output"]["results"][0]["index"], 1);
    assert_eq!(calls[2]["output"]["results"][0]["relevance_score"], 0.75);
    for call in calls {
        assert_eq!(call["trace"]["provider"]["configured_ms"], 1_700_000);
        assert!(call["trace"]["idle_ms"].is_null());
    }
    daemon.cli(&args);
    assert_eq!(text.requests.lock().unwrap().len(), 1);
    assert_eq!(embedding.requests.lock().unwrap().len(), 1);
    assert_eq!(reranker.requests.lock().unwrap().len(), 1);
    assert_eq!(
        embedding.requests.lock().unwrap()[0].1["encoding_format"],
        "float"
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    assert_eq!(
        daemon.cli(&["job", "show", "models"])["data"]["result"],
        completed["result"]
    );
    assert_eq!(text.requests.lock().unwrap().len(), 1);
}
