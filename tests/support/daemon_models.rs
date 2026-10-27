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
    NearLimit,
    Delay(u64),
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
                let delay = match mode {
                    Mode::Slow => Duration::from_millis(400),
                    Mode::Delay(seconds) => Duration::from_secs(seconds),
                    _ => Duration::ZERO,
                };
                let until = Instant::now() + delay;
                while Instant::now() < until {
                    if stopping.load(Ordering::Acquire) {
                        return;
                    }
                    std::thread::sleep(
                        until
                            .saturating_duration_since(Instant::now())
                            .min(Duration::from_millis(100)),
                    );
                }
                let response = if matches!(mode, Mode::NearLimit) {
                    json!({"choices":[{"index":0,"message":{"role":"assistant","content":"x".repeat(512*1024-128)},"finish_reason":"stop"}]})
                } else if matches!(mode, Mode::Malformed) {
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
                if !matches!(mode, Mode::Slow | Mode::Delay(_)) {
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

fn publish(path: &std::path::Path, value: &Value) {
    let temporary = path.with_extension("tmp");
    let mut file = std::fs::File::create(&temporary).unwrap();
    serde_json::to_writer(&mut file, value).unwrap();
    file.sync_all().unwrap();
    std::fs::rename(temporary, path).unwrap();
    std::fs::File::open(path.parent().unwrap())
        .unwrap()
        .sync_all()
        .unwrap();
}

fn wall_clock_case(real: bool, receipt: Option<PathBuf>) -> Value {
    use akasha_otsuge::ipc::now_ms;
    use sha2::{Digest, Sha256};
    let started_at = now_ms().unwrap();
    let started = Instant::now();
    let (success_delay, limit) = if real { (1605, 1700) } else { (1, 2) };
    let success = Endpoint::with_mode(Mode::Delay(success_delay));
    let over = Endpoint::with_mode(Mode::Delay(limit + 2));
    let mut daemon = Daemon::new();
    let receipt = receipt.unwrap_or_else(|| daemon.root.join("wall-clock"));
    std::fs::create_dir(&receipt).unwrap();
    let binary = std::env::var_os("AKASHA_ACCEPTANCE_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(BIN));
    let binary_sha256: String = Sha256::digest(std::fs::read(&binary).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let identity = json!({"schema_version":1,"binary":binary,"binary_sha256":binary_sha256,"real_wall_clock":real,"success_delay_s":success_delay,"provider_limit_s":limit,"max_attempts":1,"started_at_ms":started_at,"test_pid":std::process::id(),"scratch":daemon.root});
    publish(&receipt.join("admission.json"), &identity);
    let settings = format!(
        "[run]\nreal_wall_clock={real}\nsuccess_delay_s={success_delay}\nprovider_limit_s={limit}\n[reliability]\nmax_attempts=1\n"
    );
    let config_path = receipt.join("acceptance.toml");
    std::fs::write(&config_path, &settings).unwrap();
    std::fs::File::open(&config_path)
        .unwrap()
        .sync_all()
        .unwrap();
    let override_timeout = if real {
        String::new()
    } else {
        format!("timeout='{limit}s'\n")
    };
    daemon.configure(&format!("[runtime]\nmax_concurrent_jobs=2\n[models.success]\nkind='text'\nendpoint={:?}\nmodel='controlled-success'\nlocal=true\n{override_timeout}[models.over]\nkind='text'\nendpoint={:?}\nmodel='controlled-timeout'\nlocal=true\n{override_timeout}",success.address,over.address));
    daemon.start();
    let mut jobs = serde_json::Map::new();
    for profile in ["success", "over"] {
        let plan = json!({"calls":[{"profile":profile,"input":{"kind":"text","prompt":"controlled deadline response","max_output_tokens":32}}]});
        let path = daemon.root.join(format!("{profile}.json"));
        std::fs::write(&path, plan.to_string()).unwrap();
        let accepted = daemon.cli(&[
            "model",
            "run",
            path.to_str().unwrap(),
            "--idempotency-key",
            profile,
        ])["data"]
            .clone();
        assert_eq!(accepted["config_generation"], 1);
        jobs.insert(profile.into(), accepted);
    }
    await_request(&success);
    await_request(&over);
    assert_eq!(
        daemon.cli(&["job", "show", "success"])["data"]["state"],
        "running",
        "the two deadline cases must overlap"
    );
    let mut completed = serde_json::Map::new();
    loop {
        for profile in ["success", "over"] {
            if completed.contains_key(profile) {
                continue;
            }
            let job = daemon.cli(&["job", "show", profile])["data"].clone();
            if matches!(
                job["state"].as_str(),
                Some("succeeded" | "failed" | "cancelled")
            ) {
                assert_eq!(
                    std::fs::read_to_string(&config_path).unwrap(),
                    settings,
                    "acceptance config changed during frozen cases"
                );
                completed.insert(profile.into(),json!({"observed_at_ms":now_ms().unwrap(),"elapsed_s":started.elapsed().as_secs_f64(),"job":job.clone()}));
            }
            jobs.insert(profile.into(), job);
        }
        let state = json!({"identity":identity,"completed":completed,"jobs":jobs,"elapsed_s":started.elapsed().as_secs_f64()});
        publish(&receipt.join("state.json"), &state);
        publish(
            &receipt.join("progress.yaml"),
            &json!({"completed":completed.len(),"total":2,"elapsed_s":started.elapsed().as_secs_f64(),"rate_per_s":completed.len() as f64 / started.elapsed().as_secs_f64(),"eta_s":Value::Null,"active_unit":"controlled provider delay","daemon_pid":daemon.child.as_ref().unwrap().id(),"provider_requests":[success.requests.lock().unwrap().len(),over.requests.lock().unwrap().len()]}),
        );
        if completed.len() == 2 {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(limit + 30),
            "model job never reached a terminal state"
        );
        std::thread::sleep(Duration::from_millis(if real { 1000 } else { 20 }));
    }
    assert_eq!(jobs["success"]["state"], "succeeded");
    assert_eq!(
        jobs["success"]["result"]["calls"][0]["output"]["text"],
        "hello"
    );
    assert_eq!(jobs["over"]["state"], "failed");
    assert_eq!(jobs["over"]["error"], "PROVIDER_TIMEOUT");
    for profile in ["success", "over"] {
        let trace = &jobs[profile]["result"]["calls"][0]["trace"];
        assert_eq!(trace["provider"]["configured_ms"], limit * 1000);
        assert_eq!(trace["provider"]["effective_ms"], limit * 1000);
        assert!(trace["idle_ms"].is_null());
    }
    let elapsed = jobs["success"]["result"]["calls"][0]["trace"]["provider"]["elapsed_ms"]
        .as_u64()
        .unwrap();
    assert!(elapsed >= success_delay * 1000 && elapsed < limit * 1000);
    assert_eq!(success.requests.lock().unwrap().len(), 1);
    assert_eq!(over.requests.lock().unwrap().len(), 1);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    drop(success);
    drop(over);
    let result = json!({"identity":identity,"completed":completed,"passed":true,"owned_daemon_and_endpoint_threads_ended":true,"finished_at_ms":now_ms().unwrap(),"elapsed_s":started.elapsed().as_secs_f64()});
    publish(&receipt.join("verified.json"), &result);
    result
}

#[test]
fn wall_clock_harness_short_control() {
    let result = wall_clock_case(false, None);
    assert_eq!(result["passed"], true);
    assert_eq!(result["identity"]["real_wall_clock"], false);
}

#[test]
#[ignore = "1700-second real production daemon/provider acceptance; explicit receipt directory required"]
fn real_wall_clock_provider_deadlines() {
    let receipt = std::env::var_os("AKASHA_PROVIDER_RECEIPT")
        .map(PathBuf::from)
        .expect("set a fresh AKASHA_PROVIDER_RECEIPT path");
    assert!(receipt.is_absolute());
    let result = wall_clock_case(true, Some(receipt));
    assert_eq!(result["identity"]["real_wall_clock"], true);
}

#[test]
#[ignore = "bounded synthetic inference against explicitly selected GB10 guard"]
fn gb10_guard_actual_services() {
    use sha2::{Digest, Sha256};
    let receipt = std::env::var_os("AKASHA_MODEL_COMPAT_RECEIPT")
        .map(PathBuf::from)
        .expect("set a fresh AKASHA_MODEL_COMPAT_RECEIPT path");
    assert!(receipt.is_absolute() && !receipt.exists());
    std::fs::create_dir(&receipt).unwrap();
    let endpoint = "http://gb10:18009/v1";
    let mut daemon = Daemon::new();
    daemon.configure(&format!("[models.text]\nkind='text'\nendpoint={endpoint:?}\nmodel='abliterated-qwen-latest-27b-none'\nlocal=true\n[models.embedding]\nkind='embedding'\nendpoint={endpoint:?}\nmodel='qwen3-embedding-8b'\nlocal=true\ndimension=4096\n[models.reranker]\nkind='reranker'\nendpoint={endpoint:?}\nmodel='Querit/Querit-4B'\nlocal=true"));
    let binary = std::env::var_os("AKASHA_ACCEPTANCE_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(BIN));
    let binary_sha256: String = Sha256::digest(std::fs::read(&binary).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let identity = json!({"endpoint":endpoint,"binary":binary,"binary_sha256":binary_sha256,"started_at_ms":akasha_otsuge::ipc::now_ms().unwrap(),"attempts":1,"scope":"synthetic production-client compatibility; not a long-timeout or semantic-quality claim","scratch":daemon.root});
    publish(&receipt.join("admission.json"), &identity);
    daemon.start();
    let plan = json!({"calls":[
        {"profile":"text","input":{"kind":"text","prompt":"Reply with exactly OK.","max_output_tokens":256}},
        {"profile":"embedding","input":{"kind":"embedding","texts":["akasha-otsuge synthetic compatibility check"]}},
        {"profile":"reranker","input":{"kind":"reranker","query":"local memory","documents":["local traceable memory","rainy weather forecast"]}}
    ]});
    let path = daemon.root.join("compat.json");
    std::fs::write(&path, plan.to_string()).unwrap();
    daemon.cli(&[
        "model",
        "run",
        path.to_str().unwrap(),
        "--idempotency-key",
        "compat",
    ]);
    let started = Instant::now();
    let job = loop {
        let job = daemon.cli(&["job", "show", "compat"])["data"].clone();
        publish(
            &receipt.join("progress.yaml"),
            &json!({"completed":job["result"]["calls"].as_array().map_or(0,Vec::len),"total":3,"elapsed_s":started.elapsed().as_secs_f64(),"rate_per_s":Value::Null,"eta_s":Value::Null,"job_state":job["state"],"daemon_pid":daemon.child.as_ref().unwrap().id()}),
        );
        if matches!(
            job["state"].as_str(),
            Some("succeeded" | "failed" | "cancelled")
        ) {
            break job;
        }
        assert!(started.elapsed() < Duration::from_secs(6050));
        std::thread::sleep(Duration::from_millis(250));
    };
    let bytes = serde_json::to_vec(&job).unwrap();
    let job_sha256: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let calls = job["result"]["calls"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let outputs:Vec<Value>=calls.iter().map(|c|json!({"profile":c["profile"],"identity":c["identity"],"error":c["error"],"trace":c["trace"],"text":c["output"]["text"],"vector_count":c["output"]["vectors"].as_array().map(Vec::len),"dimension":c["output"]["vectors"][0].as_array().map(Vec::len),"rerank_results":c["output"]["results"]})).collect();
    let report = json!({"identity":identity,"state":job["state"],"error":job["error"],"job_sha256":job_sha256,"calls":outputs,"finished_at_ms":akasha_otsuge::ipc::now_ms().unwrap(),"elapsed_s":started.elapsed().as_secs_f64()});
    publish(&receipt.join("result.json"), &report);
    assert_eq!(job["state"], "succeeded", "{}", report);
    assert_eq!(calls.len(), 3);
    assert_eq!(
        calls[1]["output"]["vectors"][0].as_array().unwrap().len(),
        4096
    );
    assert_eq!(calls[2]["output"]["results"].as_array().unwrap().len(), 2);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    publish(
        &receipt.join("verified.json"),
        &json!({"report":report,"passed":true,"owned_daemon_ended":true}),
    );
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
fn three_bounded_responses_fit_the_durable_archive_and_ipc() {
    let endpoint = Endpoint::with_mode(Mode::NearLimit);
    let mut daemon = Daemon::new();
    daemon.configure(&text_config(&endpoint));
    daemon.start();
    let call =
        json!({"profile":"text","input":{"kind":"text","prompt":"hello","max_output_tokens":8192}});
    let path = daemon.root.join("large-plan.json");
    std::fs::write(
        &path,
        json!({"calls":[call.clone(),call.clone(),call]}).to_string(),
    )
    .unwrap();
    daemon.cli(&[
        "model",
        "run",
        path.to_str().unwrap(),
        "--idempotency-key",
        "large",
    ]);
    let job = terminal(&daemon, "large");
    assert_eq!(job["state"], "succeeded");
    for call in job["result"]["calls"].as_array().unwrap() {
        assert_eq!(
            call["output"]["text"].as_str().unwrap().len(),
            512 * 1024 - 128
        );
    }
    assert_eq!(endpoint.requests.lock().unwrap().len(), 3);
    assert_eq!(
        daemon.cli(&["daemon", "status"])["data"]["state"],
        "running"
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    assert_eq!(
        daemon.cli(&["job", "show", "large"])["data"]["result"],
        job["result"]
    );
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
