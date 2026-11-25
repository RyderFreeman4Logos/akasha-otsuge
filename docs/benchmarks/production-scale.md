# Production corpus pilot

MVP-024 acceptance is tracked in [#65](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/65).
The earlier storage spike is not production-daemon acceptance. Full scale,
concurrency, vector coverage and ordinary recovery remain pending.

## Full import and disk use

The November 2026 run imported 50,001,003,051 UTF-8 bytes from
11,428,868 records. The independent input manifest matched all 2,791 units in
the native write journal. The resulting corpus contains 11,564,715 claims and 1,000,000
records selected for vector search. [Import receipts](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/65#issuecomment-7168165199)

Before the first full index, the stopped store occupied **109,707,300,864 bytes**
in allocated file blocks, or **2.194 times** the input text size.

| Files | Allocated bytes |
| --- | ---: |
| Authority value log | 99,384,467,456 |
| Authority sorted tables | 8,594,313,216 |
| Existing lexical and vector search files | 1,723,097,088 |
| Write-ahead log, manifests and other files | 5,423,104 |

The database retains both source evidence and claim revisions. In 718 sampled
pairs, the claim text matched the corresponding source span byte for byte.
These pairs identify a possible saving. The whole-store saving remains unknown.
[Storage measurements](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/65#issuecomment-7169208546)

On November 25 at 18:58 UTC, package `688ab304` accepted the first full
index job on the mirror SSD. The admission check used a threshold of
89,124,983,763 free bytes. It covered projected index growth, temporary files
and the free-space reserve. The SSD had 91,118,637,056 free bytes. The native job recorded target
sequence 23,119,806. Final index size and coverage await the closed-store
measurements. [Index admission](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/65#issuecomment-7171653817)

## Seed the corpus

`cargo build --locked --example corpus_seed` builds an offline benchmark component
using the production `Storage` and `Domain`. It takes a new absolute data directory
and a small JSON binding file, and reads newline-terminated JSON records from stdin.
Stream directly from retained licensed parquet shards; do not materialize another
whole JSONL corpus. This example is not installed with the product CLI.

The binding has `source_manifest_sha256` (64 hexadecimal characters) and a fixed
positive `observed_time_ms`. The controller must verify source hashes and freeze
the selection, license, source code and executable identities before seeding.
The binding does not itself verify a dataset or grant human approval.

Each input has `key`, `body` and `partition` (`p0` through `p7`). An optional
`vector_cohort` boolean defaults to false; unknown fields remain invalid. The body
must be nonempty and at most 65,536 UTF-8 bytes. The component records the complete
source and creates contiguous claims of at most 32,768 bytes without splitting a
UTF-8 character. Every claim cites its actual source revision and byte range.
When a UTF-8 boundary would leave a short all-whitespace tail, the seeder moves
the preceding claim's last non-whitespace character into that tail if both
claims remain nonempty and within the limit. Every source byte is retained.
An irreducible all-whitespace chunk still fails explicitly. Existing partial
imports retain their request-conflict protection; this change does not migrate
an already committed claim to different boundaries.
Rows become tool-origin, delegated facts only within the
local owner's `corpus/pN` project scopes; they are not human-confirmed memories.

`vector_cohort: true` additionally sets `branch_or_version` to `vectors` and
requires the complete input to fit 4,096 UTF-8 bytes. It remains one unchanged
claim. Configure `retrieval.embedding_scopes` with these eight project/branch
scopes to embed the cohort while retaining every other claim in the lexical
index. Scored hits require the exact scope: a vector query uses the corresponding
`vectors` branch, while a project-only query searches the null-branch records.
Changing the cohort flag on a previously
admitted binding/key conflicts, just as changing its body does.

`spikes/data/precomputed.py` supplies the local benchmark lookup. Its manifest
contains a model identity, dimension 1536 and at most 27 sources, each with
`path`, `size`, `sha256` and `layout` (`dbpedia` or `dictionary`). `Lookup` verifies
the files and indexes complete text hashes to Parquet row addresses in memory.
It rejects duplicate inputs and retains one decoded row group, limited to
64 MiB of declared uncompressed data. Per-file prefetch buffering is disabled.
No second text or vector file is created.
The controller owns the loopback HTTP server, shutdown, resource accounting
and durable progress callback. It records the helper's memory separately.
Unknown, altered or oversized inputs and changed files fail; there is no
synthetic fallback. This lookup replays retained embeddings and is not live
model inference. Its small fixture tests use synthetic vectors only.

One flushed acknowledgement follows each fully committed record. It includes
content hashes, the source revision and all memory revisions. Stable binding/key
identities make exact replay idempotent, including after a partially acknowledged
stream. Changed content conflicts; a different binding or an unrelated nonempty
store is refused. The controller must retain admissions and acknowledgements,
reconcile interruption and verify the final store; a flushed pipe alone is not a
durable controller checkpoint. The component requires exclusive offline storage
ownership and is not part of the installed daemon's public write API.

The initial pilot uses 128 real DBpedia records and 896 unique FineWeb chunks,
then the actual daemon CLI for indexing and current/lexical reads. It measures
the source-to-memory expansion, complete current-claim hash agreement, eight
partition witnesses, resource status and authority/index disk allocation before
admitting larger work. It does not claim full index-body coverage from row counts,
file-cache coldness, ANN execution, 100 clients or full-scale latency thresholds.

The first pilot passed on 2026-11-11 with seeder source `b191b01` and the unchanged
production daemon from reviewed `8442871`: 4,449,979 unique source bytes became
1,042 memory chunks. Every current claim hash and eight partition witnesses
matched. Seeding took 3.798 s and indexing 32.330 s. Authority allocated
12,750,848 bytes; search allocated 149,250,048 bytes across 3,843 files and 1,109
native versions. The final measured daemon RSS was 463,224,832 bytes; owned
cgroups reported zero OOM kills. These are one pilot's observations, not an
extrapolated scale result. The stopped disposable store and socket directory
were removed after receipts and all process exits were verified.

Both lexical and vector sync previously executed a native merge for every row.
The successor retains the 32-transaction authority window and per-sequence
completions, but coalesces native writes and removals by key. Pending Arrow data
flushes at one thirty-second of the configured memory budget (128 KiB through
4 MiB), or 256 distinct keys; an arriving row may cross the byte threshold.
Native engine working memory remains subject to the separate service limits.
Current-state checks still precede each read and progress publication. Two
native regressions first observed 18/19 versions for 16 lexical/vector rows;
the repaired paths stay within the eight-version test budget, including index
creation. The [paired real-corpus run](results/mvp024-production-pilot/README.md)
also passes with byte-identical inputs, acknowledgements and full claim oracle:
indexing took 10.559 s, search allocated 86,781,952 bytes in 912 files and 132
native versions. The RSS sampled peak increased to 489,836,544 bytes. This single
pair does not establish a memory improvement or full-scale operating envelope.
Both disposable databases were removed after verification. The next committed
change prunes old native versions only when the builder exclusively owns the
index. A query holding the index defers reclamation until a later idle sync;
authoritative history is preserved. The [third paired pilot](results/mvp024-native-prune/README.md)
passes the identical inputs and oracles with 5,332,992 allocated search bytes,
77 files and one native version, taking 10.491 seconds to index. This small
measurement does not establish full-scale growth, ANN or concurrent-load bounds.

The next 8,192-record pilots both fail before completing indexing. Their identical
35,875,218 unique source bytes produce 8,322 expected claims; last published
index progress is 3,140 and 3,380 claims. Both exceed the 640 MiB sampled RSS
limit and hit the unchanged 10-second status IPC deadline. Adding native merge-key
indexes passes its causal regression but does not resolve this scale failure.
The [failed scale comparison](results/mvp024-native-prune/README.md) preserves
all inputs, failures, memory breakdown and cleanup evidence. A later allocator
observation supports retained free arenas but failed its disk census and had
unavailable cgroups because the preload also reached helper processes. A control
without the sampler, changing only the daemon's mmap threshold, still failed
`job show` at 6,270/8,322 with RSS 679,796,736 bytes under effective original
limits. Neither completed the output oracles; the allocator setting alone is
insufficient. Pinned-source inspection also found that FTS creation limits are
not persisted for later default optimization. Effective update limits still
need repair and verification before scaling; partial progress is not acceptance.

On the merged maintenance source, an FTS1/32-only control again failed at
3,622/8,322 with sampled RSS 692,056,064 bytes. Adding a two-arena allocator
setting completed all current-claim checks and eight scoped witnesses under the
original limits: indexing took 203.635 seconds with sampled RSS peak 406,609,920
bytes. A separate 1 MiB FTS partition control also completed, taking 795.438
seconds with peak 282,107,904 bytes. The
[complete control archive](results/mvp024-native-memory/README.md) retains the
failure, successful output checks, all resource samples and exact cleanup.
These are single tuning observations. Commit `92031ea` then passed the same
corpus using the release daemon's own startup defaults, with no externally
injected allocator/FTS settings. All 8,322 claims and eight witnesses passed;
indexing took 218.891 seconds, and all 197 observations were complete under the
original limits. Sampled RSS maximum was 414,588,928 bytes and P99 411,643,904,
with zero sampled OOM kills. Its exact stopped store/socket were removed after
hash and consumer checks. This bounded production result does not replace the
unchanged full-scale acceptance.

The final canonical/install release artifact at `b83cf3d` also passed the same
corpus: all 8,322 claim checks and eight witnesses, 212.304 index seconds, and
198 complete enforced samples. Sampled RSS maximum was 418,988,032 bytes and P99
413,306,880, with zero sampled OOM kills. Its disposable store/socket were removed.
PR #71 merged as `96e2b5f` after exact-source gate, independent review and CI;
the [final archive](results/mvp024-native-memory/README.md#final-artifact-and-publication)
preserves the reviewer UID-prerequisite failure and separate successful parent
and CI acceptance. Full coverage/load/recovery remains pending.

All 42 retained compressed shards were rehashed on 2026-11-11: 43,986,181,099
bytes matched the pinned source inventory. A separate text-only shape census
found exactly 1,000,000 DBpedia rows, 37 with title-plus-text over 4,096 bytes
(maximum 12,769). Those published whole-document embeddings differ from the
current 4,096-byte chunk/mean policy for those rows. Resolve and verify that
input/model/generation contract before claiming million-vector acceptance;
do not feed a whole-document embedding as if it represented a shorter chunk.

The canonical workspace test command includes the example's real Domain tests:
UTF-8 evidence and exact replay, acknowledged-prefix recovery/conflicting input,
and bounded frame reads. Source shape checks and a pilot are preparation, not
closure of the unchanged acceptance specification.

The [cohort evidence](results/mvp024-vector-cohort/README.md) records a bounded
production run with 8,229 sources, 8,359 current claims and 1,061 retained vectors.
All current claim and vector input/output checks passed. Its initial keyword
witness used the wrong branch and failed; the unchanged exact-scope contract was
correct. Reopening the same store completed eight corrected keyword witnesses
and eight vector self hits without reimport or reindex. The original failure,
different release artifact identities, separate helper memory measurements and
exact disposable-store cleanup remain recorded. This does not establish the
million-vector, full lexical, load or recovery acceptance.
