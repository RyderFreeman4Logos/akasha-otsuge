# Native 100-client storage workload

Run this workload against a dedicated test daemon and a separately identified
local embedding provider. It appends 4-KiB capture events and submits conflicting
patches to four specified memories. Every run needs a fresh request namespace
and an empty output path directly under the repository's SSD-backed
`.spike-work/` directory.

From the repository root, with Python 3.11 or newer on Linux:

```sh
python3 -m scripts.native_workload plan.json interval.toml .spike-work/native-workload-low
```

The daemon must allow its own capture input path
`.spike-work/native-workload-low/transcripts` in `access.read_roots` (absolute
path). Seed the store and sync the lexical and vector indexes before starting
the interval. The lexical scope must use keyword search. The hybrid scope
must grant embedding access and its queries must exist in the local provider.

The run plan JSON includes these fields:

| Field | Contents |
| --- | --- |
| `run_id` | Unique identifier, 1–64 ASCII letters, digits, underscores or hyphens. |
| `socket`, `daemon_config`, `scale_receipt` | Absolute paths to the Unix socket, effective daemon TOML, and the seed/coverage evidence for this store. |
| `daemon`, `lookup` | Linux process identities with `pid` and `start_ticks` from procfs. |
| `daemon_sha256`, `daemon_config_sha256`, `scale_receipt_sha256` | SHA-256 of the running executable and the two admitted files. |
| `concurrent_index` | Set to `true` for the mixed-load index-build check. The runner submits an incremental sync halfway through the interval. |
| `oracles.current` | Nonempty list of stable memories, each with `id`, `revision`, `scope` and UTF-8 `claim_sha256`. |
| `oracles.lexical`, `oracles.hybrid_vector` | Lists with `query`, `mode` (`keyword` or `hybrid`), `scope`, `top_k` equal to 20, one or more `required_refs` pairs of memory ID and revision, and optional `expected_rules` and `excluded_ids`. |
| `oracles.scoped_history` | List of snapshots with `scope`, fixed sequence `at`, and `members`, a map from each of 100 distinct IDs to its expected current-memory fields at that sequence. |
| `oracles.corrections` | Four distinct writable test targets, each with `id` and `scope`. Use separate memories for stable current reads. |

Use the native scope representation, including the user, project, session
and branch/version. Freeze `expected_rules` from the seeded memories and
the query's read grant before running the workload. Each entry contains
`id`, `revision` and the rule's own `scope`. The list specifies exactly
which confirmed rules must appear. A rule may omit a project, branch or
session constraint held by the query, provided its remaining constraints
match and the grant permits reading that scope. With this list, `required_refs`
checks scored hits in the query scope. For older plans that omit the list,
a rule in the query scope can satisfy an explicit `required_refs` entry.
An empty rule list requires a result without rules. Excluded IDs apply to
both rules and hits.

With `concurrent_index` enabled, the runner records when the daemon enters
and leaves the build using the process-local `index_status.build_epoch`
counter. It accepts a write or correction only if two samples from the same
active build bracket the entire request. The sync must succeed and advance
the lexical watermark through its admitted target. If the submission reply
is lost, the runner looks up the saved job ID. The intent and terminal
receipts identify the job for later reconciliation.

Example low-memory interval:

```toml
[execution]
seconds = 1800

[reliability]
max_attempts = 1
request_timeout_s = 10
job_timeout_s = 600

[resources]
target_mib = 512
peak_mib = 640
service_total_mib = 2048
```

For a short functional check, use 55 seconds to exercise every client role and
duplicate original writes. The runner records each offer, admitted request,
outcome and resource sample. If the process stops early, keep its journals
and reconcile uncertain effects before starting a fresh interval.

Each client keeps its durable retrieval job ID in the journal. If observation
fails after submission, that client stops dispatching requests. The final report
lists operations still in flight at the interval boundary and retains their
eventual outcomes and latency. Each pair of dispatched corrections must share
one target and base revision and produce one committed revision and one
`REVISION_CONFLICT`. Partial rounds identify which participant dispatched.

The report shows offers and successful completions by role for each minute. It
checks complete steady-state minutes against the published role rates, scaled
to 60 seconds. This window includes the ten-corrections-per-minute contract.
The whole interval includes any final partial minute. A completion after the
interval appears in the late count and the whole-run latency distribution.

Each one-second sample records the daemon status sequence and an authorized
`index_status` observation with `include_queue=true`. The observation reads
the bounded active-job index, reports queued and running counts and counts the
actual UTF-8 JSON bytes of waiting job records. Admission reservations use a
separate accounting rule. Index lag is the observed authority sequence minus
the watermark for each named lexical, configured vector and active vector
generation. Sampler calls have their own request IDs and throughput excludes
them. A missing queue or generation measurement leaves a coverage gap.

The initial status check must confirm the daemon and its resource limits.
During the interval, a status reply of `RESOURCE_BUSY` records the sample
number, error code and monotonic time in `metrics.jsonl`. Scheduled offers,
the queue query, procfs readings and fresh cgroup OOM counters continue.
That sample has a null daemon-group measurement and status sequence. Its
queue observation contributes to the high-water counts when available.
Resource coverage and the paired status/index coverage fail for the missing
sample. RSS statistics use only measured daemon groups.

Once the daemon and all other store users have exited, verify the durable
raw and normalized bytes for the interval's acknowledged originals:

```sh
cargo build --locked --example capture_verify
python3 -m scripts.native_workload.verify .spike-work/native-workload-low target/debug/examples/capture_verify
```

The verifier reopens the same store, matches journal identities to the
offered schedule, and checks every accepted capture against its publication
receipt. A completed interval that missed a performance target can still verify
its acknowledged originals. The output retains that interval verdict and its
gaps. An ambiguous prepared write or inconsistent journal requires reconciliation.
A short run validates this path at its recorded scale. Corpus
coverage, historical revisions, ANN recall, overload and recovery have
separate acceptance criteria in [the storage specification](../spec/08.md).
