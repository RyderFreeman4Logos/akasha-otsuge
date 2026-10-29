use super::*;
use std::net::TcpListener;
use std::sync::{Arc, Mutex, atomic::AtomicBool};

pub(crate) struct Endpoint {
    pub(crate) address: String,
    pub(crate) requests: Arc<Mutex<Vec<(String, Value)>>>,
    pub(crate) received_at: Arc<Mutex<Vec<Instant>>>,
    pub(crate) responded_at: Arc<Mutex<Vec<Instant>>>,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
#[derive(Clone, Copy)]
pub(super) enum Mode {
    Normal,
    Malformed,
    Disconnect,
    Slow,
    RateLimited,
    NearLimit,
    Delay(u64),
    Retrieval,
    Extraction,
}
impl Endpoint {
    pub(crate) fn pause(&self) {
        self.paused.store(true, Ordering::Release);
    }
    pub(crate) fn resume(&self) {
        self.paused.store(false, Ordering::Release);
    }
    pub(crate) fn retrieval() -> Self {
        Self::with_mode(Mode::Retrieval)
    }
    pub(crate) fn extraction() -> Self {
        Self::with_mode(Mode::Extraction)
    }
    pub(super) fn new() -> Self {
        Self::with_mode(Mode::Normal)
    }
    pub(super) fn with_mode(mode: Mode) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let saved = requests.clone();
        let received_at = Arc::new(Mutex::new(Vec::new()));
        let responded_at = Arc::new(Mutex::new(Vec::new()));
        let arrivals = received_at.clone();
        let responses = responded_at.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let paused = Arc::new(AtomicBool::new(false));
        let held = paused.clone();
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
                arrivals.lock().unwrap().push(Instant::now());
                saved.lock().unwrap().push((path.into(), body.clone()));
                while held.load(Ordering::Acquire) {
                    if stopping.load(Ordering::Acquire) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
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
                let response = if matches!(mode, Mode::Retrieval) {
                    match path {
                        "/v1/embeddings" => {
                            let dimension = if body["model"] == "vectors-3" { 3 } else { 2 };
                            let data: Vec<_> = body["input"].as_array().unwrap().iter().enumerate().map(|(index,text)| {
                                let text = text.as_str().unwrap();
                                let offline = text.contains("offline") || text.contains("network") || text.contains("离线");
                                let mut embedding = vec![if offline { 1.0 } else { 0.0 }, if offline { 0.0 } else { 1.0 }];
                                match body["model"].as_str() {
                                    Some("invalid-zero") => embedding = vec![0.0, 0.0],
                                    Some("invalid-overflow") => embedding = vec![1e300, 1.0],
                                    Some("invalid-mean") => embedding = vec![if text.starts_with('x') { 1.0 } else { -1.0 }, 0.0],
                                    _ => (),
                                }
                                if dimension == 3 { embedding.push(0.0); }
                                json!({"index":index,"embedding":embedding})
                            }).collect();
                            json!({"data":data})
                        }
                        "/v1/rerank" => {
                            let results: Vec<_> = body["documents"].as_array().unwrap().iter().enumerate().map(|(index,text)| {
                                json!({"index":index,"relevance_score":if text.as_str().unwrap().contains("offline") { 0.9 } else { 0.2 }})
                            }).collect();
                            json!({"results":results})
                        }
                        _ => panic!("unexpected retrieval path"),
                    }
                } else if matches!(mode, Mode::Extraction) {
                    let content = json!({"schema_version":1,"candidates":[{
                        "kind":"decision","scope":{"user":format!("uid:{}",rustix::process::geteuid().as_raw()),"project":"alpha","branch_or_version":null,"session":null},
                        "claim":"Use keyword search when offline.","rationale":"A conditional proposal.",
                        "conditions":["offline"],"assumptions":[],"sources":[{"source":0,"start":0,"end":36}],
                        "valid_from_ms":null,"valid_until_ms":null
                    }]}).to_string();
                    json!({"choices":[{"index":0,"message":{"role":"assistant","content":content},"finish_reason":"stop"}]})
                } else if matches!(mode, Mode::NearLimit) {
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
                responses.lock().unwrap().push(Instant::now());
                let sent = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.len(),
                    response
                );
                if !matches!(mode, Mode::Slow | Mode::Delay(_))
                    && let Err(error) = sent
                {
                    // Revoking a live retrieval grant closes the held response.
                    assert!(
                        matches!(mode, Mode::Retrieval)
                            && matches!(
                                error.kind(),
                                std::io::ErrorKind::BrokenPipe
                                    | std::io::ErrorKind::ConnectionReset
                            ),
                        "{error}"
                    );
                }
            }
        });
        Self {
            address,
            requests,
            received_at,
            responded_at,
            stop,
            paused,
            worker: Some(worker),
        }
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}
