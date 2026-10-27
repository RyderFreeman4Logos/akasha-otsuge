# Model calls

`model run PLAN_JSON --idempotency-key ID` accepts a durable job and returns its
handle. Use `job show ID` for state/results and `job cancel ID` to cancel. The
10-second CLI/IPC exchange does not limit background execution. Read permissions
are checked at admission, between stages and when reading results.

Model profiles are independent. Each endpoint is the API **base URL**, including
its version prefix. The client appends `chat/completions`, `embeddings` or `rerank`.
For example, replace these local service addresses and model identifiers:

```toml
[models.text]
kind = "text"
endpoint = "http://127.0.0.1:8000/v1"
model = "your-chat-model"
local = true
# credential_ref = "env:LOCAL_MODEL_TOKEN"

[models.embedding]
kind = "embedding"
endpoint = "http://127.0.0.1:8001/v1"
model = "your-embedding-model"
local = true
dimension = 4096 # Must match this exact service/model.

[models.reranker]
kind = "reranker"
endpoint = "http://127.0.0.1:8002/v1"
model = "your-reranker"
local = true
```

Example plan, with three sequential independent calls:

```json
{"calls":[
  {"profile":"text","input":{"kind":"text","prompt":"Reply with OK.","max_output_tokens":128}},
  {"profile":"embedding","input":{"kind":"embedding","texts":["one","two"]}},
  {"profile":"reranker","input":{"kind":"reranker","query":"one","documents":["one","two"]}}
]}
```

Text uses non-streaming OpenAI-compatible chat with one complete assistant text
choice. A token-limit finish is `PROVIDER_OUTPUT_TRUNCATED`. Embeddings explicitly
request float encoding and validate count, original indexes, configured dimension
and finite values. Reranking uses the
[vLLM rerank contract](https://docs.vllm.ai/en/stable/models/pooling_models/scoring/),
validates unique original indexes and finite scores, and preserves the service's
raw scores and returned order. The client does not impose one model's score scale
on another model. Embedding wire shape follows the
[vLLM embedding contract](https://docs.vllm.ai/en/stable/models/pooling_models/embed/).

Plans contain at most three calls and 16 KiB of serialized input. A batch has at most
eight nonempty texts/documents. Each response is bounded to 512 KiB before parsing.
Text output budgets must be explicit and between 1 and 8192 tokens. The shared
daemon client admits four concurrent physical calls and starts at most one every
100 ms; waiting uses a separate queue budget. Runtime job intake retains its own
configured caps. HTTP 429 is an explicit `PROVIDER_RATE_LIMITED` failure; no call is
automatically replayed. TLS certificate verification remains enabled. Environment
proxies, redirects and transport retries are disabled. Only the configured
`env:NAME` credential is read, and secrets/upstream errors never enter diagnostics.

Each accepted job pins the entire configuration generation. Reload affects new
jobs; accepted queued/running jobs retain their original routes and budgets.
Default provider execution is 1700 seconds, measured monotonically from request
dispatch through complete response validation. Connect-only timeout is 10 seconds;
idle/first-response/read-gap restriction is disabled unless explicitly configured.
An enabled idle restriction is reported separately and does not renew the total
provider deadline. The automatic model-workflow budget sums all stage request
budgets, bounded admission waits and cleanup. An explicit workflow budget is
reported and can be the limiting layer. Queue time does not reduce an individual
provider's configured execution budget.

Results include route/model identity, configured/effective/elapsed/remaining
provider and workflow clocks, queue time and the limiting/triggered layer. A
not-yet-started provider has no start trace. Cancellation drops the owned HTTP
future; it does not prove a remote inference engine stopped its own computation.
Graceful shutdown or crash leaves an in-flight model job failed with
`PROVIDER_INTERRUPTED`. Queued jobs remain recoverable. The same idempotency key
returns the existing job; a changed plan conflicts. Running external requests are
never automatically replayed after an ambiguous interruption. Partial completed
stages remain archived. Results are chunked into immutable Storage records with
hash/revision checks, so embedding arrays do not overflow one authority record.

Current fixtures cover independent endpoints, bad output, disconnects, slow
responses, throttling, cancellation, shutdown/crash recovery, endpoint reload and
virtual 1699/1700-second boundaries with three serial stages. Actual GB10 inference
and the production daemon's near 1700-second wall-clock acceptance remain pending
in #9. `local_long_timeout_verified` therefore remains false; a fixture receipt
must never certify a different provider or a future host Runner.
