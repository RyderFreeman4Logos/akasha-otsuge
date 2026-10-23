# Storage experiment contract — MVP-003

Status: criteria frozen before candidate measurement on 2026-10-22. No backend
has passed or been selected. [Issue #3](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/3).
Authoritative requirements: [storage](../spec/05.md), [resources](../spec/08.md).
This contract applies unchanged to both permitted candidates and to MVP-024.

## Candidate order and decision

1. HelixDB embedded, repository tag `v3.5.1`, Rust SDK package `3.0.0`.
2. Only after a demonstrated hard failure: SurrealKV `1.0.0` authority and
   LanceDB stable `0.40` derived indexes. Lock the exact patch/commit before use.

Record exact commits, licenses, Cargo.lock and commands. Rust APIs must actually
provide the required controls. A build failure is a reproducibility failure,
not a measured performance failure. Adapter defects must be investigated before
attributing them to the engine. Both candidates failing blocks dependent work;
no third backend or reduced acceptance is implicitly authorized.

The temporary Rust experiment is separate from the product workspace. Only one
passing backend may enter production. Small functional probes are diagnostic,
never substitutes for the scale experiment.

## Data eligibility and manifest

Before measuring performance, freeze and hash a manifest containing source URLs,
licenses, content hashes, generation seed, document/memory/revision counts,
UTF-8 byte counts, vector model/revision/dimension/count, lexical/vector coverage,
excluded bytes with reasons, disjoint hot/cold partitions, oracle version and
disk occupancy. Source material must be public and licensed for this use; no
private user sessions or existing memory stores are benchmark input.

- At least **50,000,000,000 actual UTF-8 content bytes**. No sparse files, filler,
  duplicate copies or one repeated string counts toward the total.
- All eligible text declared searchable must actually be indexed. Report
  compression separately from content size and all exclusions explicitly.
- At least **1,000,000 distributed, nonzero/nonidentical vectors** from the
  actual chosen embedding model and dimension, covering hot and cold partitions.
  Precomputed vectors are allowed; record their provenance and unvectorized text.
- At least **100,000 deterministic changes**, covering revisions, retractions,
  restorations and scope exceptions. The oracle checks identities and content,
  not just counts, at known commit sequences and after restart.
- Diagnostic stages grow to the full corpus; passing smaller stages cannot
  close #3. Preserve the same manifest/oracle for the production protocol in #24.

## Environment and resource profiles

Capture distribution/kernel, Rust/compiler, CPU/affinity, RAM, filesystem, SSD,
available space, swap, cgroup delegation and competing load before each run.
Use isolated owned directories; never clear host caches or stop other services.
Reserve disk space for authority, current and next derived generation, scratch,
and commit completion before ingestion/reindex; report an insufficient-space
blocker rather than filling the user's filesystem.

| Profile | Steady RSS P99 | Maximum sampled RSS |
| --- | --- | --- |
| low (required) | 512 MiB | 640 MiB |
| middle (optional diagnostic) | 1024 MiB | 1280 MiB |
| high (required) | 2048 MiB | 2048 MiB |

Measure the full database/index process tree, including helpers and queues;
report harness memory separately. Sample RSS/PSS, I/O, queue depth/bytes, lag and
cgroup memory/events every second. Configure engine caches, memtables, pending
work and build buffers explicitly, leaving room for request work/runtime. Cache
limits alone do not prove RSS bounds. One build slot and bounded query/write
admission are allowed; 100 clients need not mean 100 simultaneous ANN executions.

Use dedicated delegated cgroups if available. Otherwise state that hard limits
are unavailable, use software admission/process-tree measurement and do not
claim cgroup protection. Distinguish process-cold from file-cache-cold reads;
restarting the harness alone is not a cold database.

## Workload and pass thresholds

100 independent clients, each with a unique request ID and at most one heavy
request in flight. Some share logical memories to exercise real conflicts.
Run **at least 30 minutes per required profile** at these aggregate rates:

| Operation | Minimum rate | P95 bound |
| --- | --- | --- |
| Durable writes, approximately 4 KiB each | 10/s | 5 s |
| Current structured reads | 2/s | 5 s |
| BM25, top 20 | 1/s | 60 s |
| ANN, top 20 | 0.2/s | 60 s |
| Fixed-scope historical pages, 100 entries | 0.2/s | 10 s |
| Corrections/retractions | 10/minute | all reach a definite result |

Also run a simultaneous 100-client burst and overload. Include all errors and
timeouts in result accounting; rejecting normal steady requests is not success.
Queues are bounded by bytes and entries, progress is fair, and overload drains
within five minutes after input stops or identifies the exact stuck tasks.
No OOM, unbounded growth, lost durable acknowledgements or duplicate logical
application. Current/history/scope/retraction/idempotency oracle accuracy must
be **100%**. Measure ANN recall separately; approximate search never permits
retracted content to leak. Verify coverage across every corpus partition.

## Transaction and recovery probes

The same authority transaction appends revision, checks/switches head, writes
request hash/receipt, appends derived work and advances commit sequence. External
immutable blobs, if chosen, are written, checked and synced before references
commit. Decide one raw-text layout in the final ADR. No models/network inside
transactions. Request ID reuse with changed content must conflict.

Historical `get(id, at)` and `state(scope, at, cursor, limit)` survive restarts
without replaying all events per query. Receipt stages distinguish queued,
committed and generation-specific indexed. Derived watermarks cannot jump gaps.
Current results revalidate against latest authoritative head/scope before return;
historical results retain their fixed commit point and check current permission.

Use real forced process exits before commit, after durable commit/before reply,
after index application/before watermark, and around generation activation.
Check receipts and oracle after every reopen. SIGKILL demonstrates process-crash
recovery only, not power-loss safety.

Required scenarios also include:

- First low-profile open of the full database; large pending text/vector backlog
  and interrupted builds reopened directly at 512 MiB.
- Reindex while ordinary writes continue; failed builds retain the old readable
  generation; model/dimension identities never mix.
- 2 GiB to 512 MiB drain/reopen with new admission restricted and old work's
  original deadlines preserved. Report requested/effective budget and draining.
- Corrections concurrent with query/build; generation corruption and rebuild;
  disk exhaustion in isolated fixtures; authority corruption is an explicit error.
- Child failure/resource exhaustion, invalid config and competing reloads.
  Full real-provider 1700-second acceptance stays assigned to #9; it is not
  claimed by storage fixtures.

## Evidence and decision format

Preserve commands, exit status, executable/lock/manifest hashes, raw JSONL samples,
latency records (including errors), oracle mismatches, disk categories and a
summary for every run. Report `passed`, `failed`, `not_run` or `blocked` separately.
The final ADR identifies one passing backend, eliminated alternatives, verified
durability boundaries and remaining limits. No backend is selected solely from
documentation, source comments or projected resource use.
