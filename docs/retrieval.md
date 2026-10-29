# Disk retrieval and progressive originals

MVP-010 adds retrieval jobs to the existing private Linux daemon. The indexes
contain **active Memory records**, not every captured message, source or historical
revision. `searched_content: "active_memories"` makes that boundary explicit.
The full acceptance queue and publication state are in [TODO](plan/todo.md).

## Commands

Start the daemon using [the management commands](daemon.md). Write an absolute
scope file such as the following, replacing the user with the daemon's kernel UID:

```json
{"user":"uid:1001","project":"alpha","branch_or_version":null,"session":null}
```

```sh
akasha-otsuge index sync --idempotency-key build-1
akasha-otsuge index rebuild --idempotency-key rebuild-1
akasha-otsuge job show build-1
akasha-otsuge index status
akasha-otsuge search '离线 load_config v2.3.1' --scope /absolute/scope.json --idempotency-key query-1
akasha-otsuge job show query-1
akasha-otsuge receipt replay 'search/<request-hash>'
akasha-otsuge show SOURCE_ID --revision COMMIT_SEQ --range BYTE_START BYTE_END
```

Add `--config` or `--socket` before the command when using nondefault paths.
Sync/search return accepted job handles; inspect the job for its terminal state.
`job cancel ID` uses the same durable cancellation path as other jobs. Reusing an
idempotency key returns its original operation; use a new key for a new sync or
query after content/configuration changes. `job show` returns the archived output
with `retrieval_mode: receipt_replay`, not a fresh search. `receipt replay` writes
the exact archived bytes without adding a newline and records transport delivery.

To pause ongoing indexing, cancel its job; completed prefix checkpoints remain.
Set `runtime.paused = true` and reload to stop new admissions/claims, including
across daemon restart. Restore it to false and submit `index sync` with a new key
to resume from the durable prefix. A running model call is not replayed implicitly.

`index rebuild` starts a fresh lexical/vector generation from authoritative
records, including when completed native index files are missing. It requires no
active reader of the index cache (`RESOURCE_BUSY` otherwise). The generation
change is durable before native tables change. Old owned native index tables are
retired, while source/memory revisions, receipts and progress history remain.
Queries report not-ready/partial coverage until the new generation catches up.
After interrupted rebuilding, a new sync resumes its saved prefix; another
explicit rebuild also reclaims any tables left by an interrupted retirement.
This maintenance operation does not promise uninterrupted old-generation service.

`show` accepts UTF-8 **byte** boundaries and a half-open range. It returns actual
`start`, `end`, `requested_end`, `total_bytes`, `truncated`, output and receipt ID.
The character budget can shorten the requested range without splitting a code
point. A bad boundary, reversed range or out-of-bounds end is `INVALID_RANGE`.
Follow the returned end to read the next piece. A captured event can exceed one
Source record: its immutable normalized source spans identify the ordered pieces;
each piece uses this same command and retains its own exact hash/revision.

## Query and authority

The selected [storage ADR](benchmarks/storage-decision.md) supplies LanceDB's native
disk ICU full-text index and scope B-tree. Ordinary scored hits use the exact
requested scope. Applicable human-confirmed decisions, preferences and procedures
from that scope and its ancestors are returned separately in `rules`, even when
their wording does not match the query. Global and local conflicting rules remain
separate evidence; the search does not silently resolve the conflict. A narrow
read grant never authorizes an ancestor outside that grant.

With `retrieval.embedding`, a real query embedding searches a model-bound native
cosine IVF-flat index. Lexical and vector ranks are combined with reciprocal rank
fusion (`1/(60+rank)`). With `retrieval.reranker`, the configured endpoint ranks
the bounded candidates. Providers use the existing [model contracts](providers.md).
Embedding failures can yield keyword results with `embedding_unavailable`;
reranker failures retain the existing ranking with `reranker_unavailable`.
Cancellation, workflow expiration and interrupted calls fail the job. A missing
initial lexical index is `INDEX_NOT_READY`, not an empty successful search.

Every candidate is checked against the current authoritative Memory revision,
scope, lifecycle, current read grant and validity window. Stale physical rows do
not make retired or corrected records current. Publication rechecks the selected
revisions, all evidence/approval spans, permissions and wall-clock validity.
Permission changes during a provider call can therefore fail the job without
publishing its old output. Historical views remain available through the
[history commands](history.md), subject to current permissions.

Search output contains the actual items/rules, query/scope, ranking mode, provider
identities/traces and coverage. The RecallReceipt binds those exact output bytes
and hash to selected revisions, source spans, read/validation sequences, accepted
configuration and **actual lexical/vector generation**. Global index diagnostics
are returned only with an owner-wide read grant. Such receipts retain that owner-wide
requirement for every later show/replay, even if their query scope is narrower.
A scoped search otherwise emits
`index: null`. The standalone `index status` requires that owner-wide grant.

## Coverage and durable progress

Sync pins the authority target sequence at admission. It rechecks the live owner
grant for each record read, after provider queueing, and before progress or final
activation. Narrowing/revoking the grant interrupts ongoing calls and prevents
subsequent records from being sent. Management stays available during model waits.
Lexical and vector indexes
maintain separate contiguous prefixes. Every covered outbox entry has a durable
completion reason: non-Memory entries are `not_required`; Memory revisions are
`indexed`, `superseded` or `inactive`. A batch publishes its completion ledger and
watermark atomically **after** physical index updates. Replaying a crash before
publication replaces the same row keys; a completed prefix does not rewrite them.
A failed embedding stores its call error/trace without advancing that prefix,
including finite-f64 conversion overflow and zero/invalid pooled cosine vectors.

`index status` reports requested-generation progress, its model identity and
dimension, the active vector generation/configuration, and `building`. Search
marks concurrent index mutation with `index_building`. A bounded outbox-tail
check can distinguish unrelated metadata writes from Memory lag; a tail larger
than 256 commits is conservatively `index_lag`. No count is a full-source coverage
claim or an ETA. `no_hit` applies to this bounded active-Memory query only; it
does not mean the user has no preference.

Changing embedding identity/dimension creates a separate generation. Queries pin
the effective generation and its provider configuration at admission. The old
generation remains usable until both candidate prefixes reach the fixed sync
target and the active pointer commits atomically. In the meantime results carry
`generation_pending`; a generation with no completed prefix is explicitly pending.
Queries can run during indexing when the configured scheduler has spare slots.
The default one-job concurrency cap still queues them normally.

A daemon restart fails ambiguous interrupted retrieval/model work instead of
automatically resending provider calls. Submit a fresh sync key to resume the
durable prefix. Missing native tables with a nonzero watermark fail
`INDEX_MISSING`; incompatible schemas/state fail closed. Do not delete authority
records or reset a watermark manually to hide a damaged index.

## Bounds and storage

Configuration validates `top_k` 1–32, `max_candidates` 1–100, `top_k` no larger
than the candidate budget, and embedding dimension 1–65536. Query text is at most
2048 bytes. Rule scans and native streams are bounded; incomplete scans are
reported. The original-character budget covers claims, rationales, conditions and
assumptions; the returned source-span union is at most 64 and serialized output
at most 128 KiB. Whole entries are dropped with an explicit budget gap/truncation
receipt instead of fabricating partial citations. The internal rule-scan page
allows 256 KiB so a maximum legal 128 KiB record and its revision envelope can
fit. Public page budgets and final search output limits are unchanged.

Reranker inputs use at most eight documents per call, each capped at 4096 UTF-8
bytes. The actual serialized provider request must fit 32768 bytes; escaping can
reduce that batch further. Clipped inputs add `reranker_input_truncated` while
returned Memory text remains intact. Long embedding claims are split into bounded
4096-byte UTF-8 chunks and mean pooled, with finite/nonzero dimension validation.
This preprocessing identity is part of the vector generation.

Jobs retain the original workflow clock across all stages. Explicit workflow
limits are honored. Automatic envelopes include configured queue/provider limits,
the pinned query model, the sync transaction/change/chunk maximum or worst-case
reranker calls. These are conservative safety envelopes, not runtime predictions;
there is no hidden 120-second execution cap.

There is one native Session/cache owner per daemon, with metadata/index cache and
spill bounds derived from the configured memory budget. Changing that budget
cannot open a second owner while an old request holds it (`RESOURCE_BUSY`); retry
after those requests drain. There is one index builder. Existing native FTS/ANN
indexes are incrementally optimized rather than rebuilt for every appended batch.

Native files live under `data_dir/search`. Authority revisions and receipts are
never removed by index updates. Model-change generations remain available until
an explicit quiescent rebuild retires old native tables. Repeated full-scale and
concurrent product rebuild acceptance remains #24. Per-record native merge/version overhead and whole-process memory
at 50 GB are not established by the small fixtures. The prior storage spike's
full-corpus results do not establish these production-daemon limits.

## Fixed regression baseline

The native fixture appends two immutable texts, then closes/reopens the real index:
`离线 offline query load_config v2.3.1` and
`在线 online publish send_message v3.4.2`. Each of the seven queries below must
rank the indicated source first; the test does not accept mere candidate presence.

| Query | Required first Memory |
| --- | --- |
| 离线 | first |
| offline | first |
| load_config | first |
| v2.3.1 | first |
| 在线 | second |
| send_message | second |
| v3.4.2 | second |

Public-daemon fixtures additionally cover two scopes, a semantic paraphrase,
conflicting global/local rules, stale retirement/correction, live grant shrink,
99,000-byte captured originals, exact output/receipt replay, provider unavailability,
long reranker inputs, source/output budgets and concurrent generation switching.
Two real SIGKILL points verify lexical prefix recovery. The local HTTP endpoint
is a controlled protocol fixture; its tiny vectors/scores do not measure a real
model's retrieval quality. The sourced 60-task evaluation is frozen in #11 before
#16 prompt tuning, and real product quality acceptance remains #27.

```sh
cargo test --locked --lib retrieval::tests -- --nocapture
cargo test --locked --test daemon retrieval_tests -- --nocapture
cargo test --locked --test domain --test config
bash scripts/check.sh
```

The [hashed development receipts](benchmarks/results/mvp010/README.md) retain
failed and successful runs with their actual commands and limits. The
[review repair receipts](benchmarks/results/mvp010-review-fixes/README.md) preserve
eight additional public regressions and two native authority controls.

Focused passes, the final committed-HEAD gate, independent review, hosted CI and
issue closure are separate evidence; [progress](plan/progress.md) records them.
