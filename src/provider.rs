//! Bounded OpenAI-compatible chat/embedding and vLLM-compatible reranking calls.
use std::future::Future;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{Mutex, Semaphore};
use tokio::time::{Instant, timeout};

use crate::config::{
    ModelKind, ResolvedConfig,
    deadline::{Deadline, Layer},
};

pub const MAX_PLAN_BYTES: usize = 16 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 512 * 1024;
pub const MAX_CALLS: usize = 3;
const MAX_BATCH: usize = 8;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub calls: Vec<Call>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Call {
    pub profile: String,
    pub input: Input,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Input {
    Text {
        prompt: String,
        max_output_tokens: u32,
    },
    Embedding {
        texts: Vec<String>,
    },
    Reranker {
        query: String,
        documents: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct Error(pub &'static str);
impl From<crate::config::ConfigError> for Error {
    fn from(_: crate::config::ConfigError) -> Self {
        Self("CONFIG_INVALID")
    }
}
type Result<T> = std::result::Result<T, Error>;

impl Plan {
    pub fn validate(&self, config: &ResolvedConfig) -> Result<Duration> {
        if self.calls.is_empty()
            || self.calls.len() > MAX_CALLS
            || serde_json::to_vec(self)
                .map_err(|_| Error("INVALID_REQUEST"))?
                .len()
                > MAX_PLAN_BYTES
        {
            return Err(Error("INVALID_REQUEST"));
        }
        let mut budget = config.timeouts().cleanup;
        for call in &self.calls {
            call.request(config)?;
            budget = budget
                .checked_add(config.timeouts().providers[&call.profile])
                .and_then(|d| d.checked_add(config.timeouts().queue))
                .ok_or(Error("CONFIG_INVALID"))?;
        }
        if config.config().timeouts.workflow == "auto" {
            Ok(budget)
        } else {
            Ok(config.timeouts().workflow)
        }
    }
}

impl Call {
    fn request(&self, config: &ResolvedConfig) -> Result<(&'static str, Value)> {
        let model = config
            .config()
            .models
            .get(&self.profile)
            .ok_or(Error("MODEL_NOT_CONFIGURED"))?;
        let valid_text = |s: &str| !s.trim().is_empty() && s.len() <= MAX_PLAN_BYTES;
        let valid_batch = |batch: &[String]| {
            !batch.is_empty() && batch.len() <= MAX_BATCH && batch.iter().all(|s| valid_text(s))
        };
        match &self.input {
            Input::Text {
                prompt,
                max_output_tokens,
            } if model.kind == ModelKind::Text
                && valid_text(prompt)
                && (1..=8192).contains(max_output_tokens) =>
            {
                Ok((
                    "chat/completions",
                    json!({"model":model.model,"messages":[{"role":"user","content":prompt}],"max_tokens":max_output_tokens,"stream":false}),
                ))
            }
            Input::Embedding { texts }
                if model.kind == ModelKind::Embedding && valid_batch(texts) =>
            {
                Ok((
                    "embeddings",
                    json!({"model":model.model,"input":texts,"encoding_format":"float"}),
                ))
            }
            Input::Reranker { query, documents }
                if model.kind == ModelKind::Reranker
                    && valid_text(query)
                    && valid_batch(documents) =>
            {
                Ok((
                    "rerank",
                    json!({"model":model.model,"query":query,"documents":documents,"top_n":documents.len(),"return_documents":false}),
                ))
            }
            _ => Err(Error("INVALID_REQUEST")),
        }
    }

    fn output(&self, config: &ResolvedConfig, response: Value) -> Result<Value> {
        let invalid = || Error("PROVIDER_OUTPUT_INVALID");
        match &self.input {
            Input::Text { .. } => {
                let choices = response["choices"].as_array().ok_or_else(invalid)?;
                if choices.len() != 1 || choices[0]["index"] != 0 {
                    return Err(invalid());
                }
                if choices[0]["finish_reason"] == "length" {
                    return Err(Error("PROVIDER_OUTPUT_TRUNCATED"));
                }
                let text = choices[0]["message"]["content"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(invalid)?;
                if choices[0]["finish_reason"] != "stop"
                    || choices[0]["message"]["role"] != "assistant"
                {
                    return Err(invalid());
                }
                Ok(json!({"text":text}))
            }
            Input::Embedding { texts } => {
                let data = response["data"].as_array().ok_or_else(invalid)?;
                if data.len() != texts.len() {
                    return Err(invalid());
                }
                let dimension = config.config().models[&self.profile]
                    .dimension
                    .ok_or_else(invalid)? as usize;
                let mut vectors = vec![Value::Null; texts.len()];
                for item in data {
                    let index = item["index"]
                        .as_u64()
                        .and_then(|i| usize::try_from(i).ok())
                        .filter(|i| *i < vectors.len())
                        .ok_or_else(invalid)?;
                    let vector = item["embedding"].as_array().ok_or_else(invalid)?;
                    if !vectors[index].is_null()
                        || vector.len() != dimension
                        || !vector
                            .iter()
                            .all(|v| v.as_f64().is_some_and(f64::is_finite))
                    {
                        return Err(invalid());
                    }
                    vectors[index] = item["embedding"].clone();
                }
                Ok(json!({"vectors":vectors}))
            }
            Input::Reranker { documents, .. } => {
                let results = response["results"].as_array().ok_or_else(invalid)?;
                if results.len() != documents.len() {
                    return Err(invalid());
                }
                let mut seen = vec![false; documents.len()];
                let mut output = Vec::new();
                for item in results {
                    let index = item["index"]
                        .as_u64()
                        .and_then(|i| usize::try_from(i).ok())
                        .filter(|i| *i < seen.len())
                        .ok_or_else(invalid)?;
                    let score = item["relevance_score"]
                        .as_f64()
                        .filter(|v| v.is_finite())
                        .ok_or_else(invalid)?;
                    if seen[index] {
                        return Err(invalid());
                    }
                    seen[index] = true;
                    output.push(json!({"index":index,"relevance_score":score}));
                }
                Ok(json!({"results":output}))
            }
        }
    }
}

#[derive(Serialize)]
pub struct Outcome {
    pub profile: String,
    pub identity: String,
    pub output: Option<Value>,
    pub error: Option<String>,
    pub trace: Value,
}

pub struct Client {
    http: reqwest::Client,
    slots: Semaphore,
    next_start: Mutex<Instant>,
}
impl Client {
    pub fn new() -> Result<Self> {
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(10))
            .pool_max_idle_per_host(2)
            .build()
            .map_err(|_| Error("PROVIDER_CLIENT_INIT"))?;
        Ok(Self {
            http,
            slots: Semaphore::new(4),
            next_start: Mutex::new(Instant::now()),
        })
    }

    pub async fn call(
        &self,
        config: &ResolvedConfig,
        call: &Call,
        workflow: &Deadline,
        cancellation: impl Future<Output = &'static str>,
    ) -> Outcome {
        let mut provider = None;
        let mut queue = None;
        let mut queue_clock = None;
        let mut idle_triggered = false;
        let identity = config
            .config()
            .models
            .get(&call.profile)
            .map(|m| {
                crate::storage::hash(
                    &serde_json::to_vec(&(m.kind, &m.endpoint, &m.model, m.dimension))
                        .expect("JSON configuration"),
                )
            })
            .unwrap_or_default();
        let request = async {
            let (route, body) = call.request(config)?;
            if serde_json::to_vec(&body)
                .map_err(|_| Error("INVALID_REQUEST"))?
                .len()
                > MAX_PLAN_BYTES * 2
            {
                return Err(Error("INVALID_REQUEST"));
            }
            let q = Deadline::start(Layer::Queue, config.timeouts().queue, None)?;
            let wait = q.remaining().min(workflow.remaining());
            queue_clock = Some(q.clone());
            let admission = timeout(wait, async {
                let permit = self
                    .slots
                    .acquire()
                    .await
                    .map_err(|_| Error("PROVIDER_CLIENT_INIT"))?;
                let mut next = self.next_start.lock().await;
                tokio::time::sleep_until(*next).await;
                *next = Instant::now() + Duration::from_millis(100);
                Ok::<_, Error>(permit)
            })
            .await;
            queue = Some(q.trace());
            let _permit = admission.map_err(|_| {
                Error(if workflow.expired().is_some() {
                    "WORKFLOW_TIMEOUT"
                } else {
                    "QUEUE_TIMEOUT"
                })
            })??;
            if workflow.expired().is_some() {
                return Err(Error("WORKFLOW_TIMEOUT"));
            }
            if q.expired().is_some() {
                return Err(Error("QUEUE_TIMEOUT"));
            }
            let model = &config.config().models[&call.profile];
            let url = format!("{}/{route}", model.endpoint.as_str().trim_end_matches('/'));
            let mut request = self.http.post(url).json(&body);
            if let Some(reference) = &model.credential_ref {
                let name = reference
                    .strip_prefix("env:")
                    .ok_or(Error("CONFIG_INVALID"))?;
                let secret =
                    std::env::var(name).map_err(|_| Error("PROVIDER_CREDENTIAL_UNAVAILABLE"))?;
                if secret.is_empty() {
                    return Err(Error("PROVIDER_CREDENTIAL_UNAVAILABLE"));
                }
                request = request.bearer_auth(secret);
            }
            let deadline = Deadline::start(
                Layer::Provider,
                config.timeouts().providers[&call.profile],
                Some(workflow),
            )?;
            let remaining = deadline.remaining();
            provider = Some(deadline);
            let response = timeout(remaining, async {
                let mut response = idle(config.timeouts().idle, request.send()).await?;
                let status = response.status().as_u16();
                if status != 200 {
                    return Err(Error(match status {
                        401 | 403 => "PROVIDER_AUTH",
                        429 => "PROVIDER_RATE_LIMITED",
                        400 | 404 | 422 => "PROVIDER_REQUEST_REJECTED",
                        408 | 504 => "PROVIDER_UPSTREAM_TIMEOUT",
                        500..=599 => "PROVIDER_UNAVAILABLE",
                        _ => "PROVIDER_PROTOCOL",
                    }));
                }
                if response
                    .content_length()
                    .is_some_and(|n| n > MAX_RESPONSE_BYTES as u64)
                {
                    return Err(Error("PROVIDER_OUTPUT_TOO_LARGE"));
                }
                let mut bytes = Vec::new();
                while let Some(chunk) = idle(config.timeouts().idle, response.chunk()).await? {
                    if chunk.len() > MAX_RESPONSE_BYTES - bytes.len() {
                        return Err(Error("PROVIDER_OUTPUT_TOO_LARGE"));
                    }
                    bytes.extend_from_slice(&chunk);
                }
                call.output(
                    config,
                    serde_json::from_slice(&bytes).map_err(|_| Error("PROVIDER_OUTPUT_INVALID"))?,
                )
            })
            .await
            .map_err(|_| {
                Error(if workflow.expired().is_some() {
                    "WORKFLOW_TIMEOUT"
                } else {
                    "PROVIDER_TIMEOUT"
                })
            })?;
            if response.as_ref().is_err_and(|e| e.0 == "IDLE_TIMEOUT") {
                idle_triggered = true;
            }
            if let Some(layer) = provider.as_ref().and_then(Deadline::expired) {
                return Err(Error(if layer == Layer::Workflow {
                    "WORKFLOW_TIMEOUT"
                } else {
                    "PROVIDER_TIMEOUT"
                }));
            }
            response
        };
        let result = tokio::select! {
            biased;
            reason = cancellation => Err(Error(reason)),
            result = request => result,
        };
        let queue = queue.or_else(|| queue_clock.as_ref().map(Deadline::trace));
        let trace = json!({"provider":provider.as_ref().map(Deadline::trace),"queue":queue,"workflow":workflow.trace(),"idle_ms":config.timeouts().idle.map(|d|d.as_millis()),"idle_triggered":idle_triggered,"connect_ms":10000,"automatic_retries":0,"local_long_timeout_verified":false});
        Outcome {
            profile: call.profile.clone(),
            identity,
            output: result.as_ref().ok().cloned(),
            error: result.err().map(|e| e.0.into()),
            trace,
        }
    }
}

async fn idle<T>(
    limit: Option<Duration>,
    future: impl Future<Output = reqwest::Result<T>>,
) -> Result<T> {
    let result = match limit {
        Some(limit) => timeout(limit, future)
            .await
            .map_err(|_| Error("IDLE_TIMEOUT"))?,
        None => future.await,
    };
    result.map_err(|e| {
        Error(if e.is_connect() {
            "PROVIDER_CONNECT"
        } else {
            "PROVIDER_TRANSPORT"
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn exhausted_workflow_cannot_start_provider_after_ready_admission() {
        let endpoint = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let config = crate::config::resolve(
            &format!(
                "[timeouts]\nqueue='1s'\n[models.text]\nkind='text'\ntimeout='100ms'\nendpoint='http://{}/v1'\nmodel='fixture'\nlocal=true",
                endpoint.local_addr().unwrap()
            ),
            None,
            "",
            &crate::config::Paths {
                data_dir: "/unused".into(),
                socket_path: "/unused/socket".into(),
            },
        )
        .unwrap();
        let call = Call {
            profile: "text".into(),
            input: Input::Text {
                prompt: "hello".into(),
                max_output_tokens: 32,
            },
        };
        let mut outcomes = Vec::new();
        for (waiting, workflow_s, expected) in [
            (false, 1, "WORKFLOW_TIMEOUT"),
            (true, 1, "WORKFLOW_TIMEOUT"),
            (true, 10, "QUEUE_TIMEOUT"),
        ] {
            let client = Client::new().unwrap();
            let workflow =
                Deadline::start(Layer::Workflow, Duration::from_secs(workflow_s), None).unwrap();
            let held = if waiting {
                Some(client.slots.acquire_many(4).await.unwrap())
            } else {
                None
            };
            let mut request =
                Box::pin(client.call(&config, &call, &workflow, std::future::pending()));
            if waiting {
                std::future::poll_fn(|cx| {
                    assert!(std::future::Future::poll(request.as_mut(), cx).is_pending());
                    std::task::Poll::Ready(())
                })
                .await;
            }
            tokio::time::pause();
            tokio::time::advance(Duration::from_secs(2)).await;
            tokio::time::resume();
            drop(held);
            let result = request.await;
            outcomes.push((expected, result));
        }
        for (expected, result) in outcomes {
            assert_eq!(result.error.as_deref(), Some(expected));
            assert!(
                result.trace["provider"].is_null(),
                "expired workflow began a provider attempt: {}",
                result.trace
            );
        }
    }

    #[tokio::test]
    async fn cancelled_admission_keeps_its_queue_trace_without_sending() {
        let config = crate::config::resolve("[models.text]\nkind='text'\nendpoint='http://127.0.0.1:9/v1'\nmodel='fixture'\nlocal=true",None,"",&crate::config::Paths { data_dir:"/unused".into(), socket_path:"/unused/socket".into() }).unwrap();
        let client = Client::new().unwrap();
        let _full = client.slots.acquire_many(4).await.unwrap();
        let workflow = Deadline::start(Layer::Workflow, Duration::from_secs(2000), None).unwrap();
        let call = Call {
            profile: "text".into(),
            input: Input::Text {
                prompt: "hello".into(),
                max_output_tokens: 32,
            },
        };
        let (cancel, cancelled) = tokio::sync::oneshot::channel();
        let request = client.call(&config, &call, &workflow, async {
            cancelled.await.unwrap();
            "CANCELLED"
        });
        tokio::pin!(request);
        tokio::select! { biased; _ = &mut request => panic!("slots must block admission"), _ = tokio::task::yield_now() => () }
        cancel.send(()).unwrap();
        let outcome = request.await;
        assert_eq!(outcome.error.as_deref(), Some("CANCELLED"));
        assert!(outcome.trace["provider"].is_null());
        assert_eq!(outcome.trace["queue"]["configured_ms"], 300000);
    }
}
