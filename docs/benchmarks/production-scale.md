# Production corpus pilot

MVP-024 acceptance is tracked in [#65](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/65).
The earlier storage spike is not production-daemon acceptance. Full scale,
concurrency, vector coverage and ordinary recovery remain pending.

`cargo build --locked --example corpus_seed` builds an offline benchmark component
using the production `Storage` and `Domain`. It takes a new absolute data directory
and a small JSON binding file, and reads newline-terminated JSON records from stdin.
Stream directly from retained licensed parquet shards; do not materialize another
whole JSONL corpus. This example is not installed with the product CLI.

The binding has `source_manifest_sha256` (64 hexadecimal characters) and a fixed
positive `observed_time_ms`. The controller must verify source hashes and freeze
the selection, license, source code and executable identities before seeding.
The binding does not itself verify a dataset or grant human approval.

Each input has exactly `key`, `body` and `partition` (`p0` through `p7`). The body
must be nonempty and at most 65,536 UTF-8 bytes. The component records the complete
source and creates contiguous claims of at most 32,768 bytes without splitting a
UTF-8 character. Every claim cites its actual source revision and byte range.
An invalid claim, including an all-whitespace chunk, fails explicitly rather than
being silently omitted. Rows become tool-origin, delegated facts only within the
local owner's `corpus/pN` project scopes; they are not human-confirmed memories.

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
Both disposable databases were removed after verification. Further native-index
disk growth and reclamation must be addressed before full-corpus admission.

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
