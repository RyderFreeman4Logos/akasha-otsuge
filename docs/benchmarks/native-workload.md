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
| `oracles.current` | Nonempty list of stable memories, each with `id`, `revision`, `scope` and UTF-8 `claim_sha256`. |
| `oracles.lexical`, `oracles.hybrid_vector` | Lists with `query`, `mode` (`keyword` or `hybrid`), `scope`, `top_k` equal to 20, one or more `required_refs` pairs of memory ID and revision, and optional `excluded_ids`. |
| `oracles.scoped_history` | List of snapshots with `scope`, fixed sequence `at`, and `members`, a map from each of 100 distinct IDs to its expected current-memory fields at that sequence. |
| `oracles.corrections` | Four distinct writable test targets, each with `id` and `scope`. Use separate memories for stable current reads. |

Use the native scope representation, including the user, project, session
and branch/version. Example low-memory interval:

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

Once the daemon and all other store users have exited, verify the durable
raw and normalized bytes for the interval's acknowledged originals:

```sh
cargo build --locked --example capture_verify
python3 -m scripts.native_workload.verify .spike-work/native-workload-low target/debug/examples/capture_verify
```

The verifier reopens the same store, matches journal identities to the
offered schedule, and checks every accepted capture against its publication
receipt. A short run validates this path at its recorded scale. Corpus
coverage, historical revisions, ANN recall, overload and recovery have
separate acceptance criteria in [the storage specification](../spec/08.md).
