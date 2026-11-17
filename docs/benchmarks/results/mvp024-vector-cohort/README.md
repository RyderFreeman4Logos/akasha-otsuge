# Explicit vector cohort and bounded production evidence

The benchmark seeder can mark complete inputs of at most 4,096 UTF-8 bytes for
the `vectors` branch. The local lookup serves only their retained 1,536-dimensional
embeddings. Both preserve the production input limit and the existing exact-scope
retrieval contract. This is preparation for [#65](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/65),
which remains open.

`evidence.zip` contains 246 members, 3,386,205 compressed bytes, SHA-256
`af6759d4505fc4a28b1ec98155dea9ffe66b72872e90e151f8fc1b628eb1fd35`.
`manifest.json` gives each member's byte count and SHA-256. The archive retains
source census, commands, admissions, focused tests, every failed attempt, resource
samples, query outputs and cleanup receipts. It contains no raw Parquet copy or
disposable native database. Runtime paths in historical receipts refer to the
original SSD workspace; the removed pilot store cannot be reopened.

## Input identity and tests

The read-only 41-shard census verified one million distinct real DBpedia vectors.
Exactly 999,963 complete title-plus-text inputs fit the product limit; 37 do not.
The eligible text hashes are unique. A separately licensed dictionary supplement
provides 37 distinct inputs/vectors with no overlap across all original vectors
or the 10,959,509 scanned FineWeb chunks. That scan includes the full last shard,
whereas the historical 50,000,002,783-byte deduplicated corpus stopped earlier.
The historical byte total was not freshly recounted here.

DBpedia embeddings are pinned to Qdrant commit
`4a9731217921bc476a0f03544f11f22ae4903fa5` (MIT embedding data; DBpedia text
CC-BY-SA 4.0). FineWeb-Edu is pinned to
`87f09149ef4734204d70ed1d046ddc9ca3f2b8f9` (ODC-BY, with original text rights).
The dictionary source is pinned to
`b9028e30ff362937bb9799aa332f98c1b2cfcf6f` in
`conwaychriscosmo/text-embedding-3-large-english-dictionary`, which declares
Apache-2.0; its upstream license record is retained. Its selected 3,072-dimensional
vectors are shortened to 1,536, L2-normalized and stored as float32. This is a
declared retained-vector benchmark identity, not proof of equality with fresh
model inference or of a shared publisher preprocessing recipe. Complete source
URLs, file hashes, transforms and license records are in the archived manifests.

The seeder's real Domain regression failed on the old implementation, then passed
with the three existing example tests. The first attempted selector ran zero
tests and is a harness failure, not RED. The actual RED fixture was later
formatted, so its raw source bytes are not described as identical to GREEN.
The lookup and preparation suites passed ten Python tests, including actual
loopback HTTP, unknown/mutated input refusal and invalid-vector checks. These
small fixtures use synthetic vectors; the subsequent real-data runs do not.
Focused Clippy, formatting and plan checks pass. A README whitespace failure
and an ambient-Python missing-PyArrow setup failure remain separate receipts.

## Retained-column diagnostic

The initial actual-data lookup returned all 89 independently checked vectors
correctly but peaked at 625,180,672 RSS bytes, exceeding its 512 MiB contract.
Its raw PASS label and first parent verification were incorrect about resources;
the explicit resource-failure receipt withdraws that claim without rewriting
either original. The harness had checked the ceiling during admission only.

A single same-input control disabled PyArrow's per-file prefetch buffers. It
preserved all 89 input/vector witnesses and peaked at 532,344,832 bytes, narrowly
below 512 MiB. Commit `3b53674b2092e6c40a18521017ba465063789359` applies that
reader setting. This single diagnostic pair does not establish a general memory
or speed bound. The production pilot runs the lookup and independent controller
as separate processes and measures them separately.

## Production pilot and corrected queries

The pilot imported 1,024 DBpedia records, all 37 supplement records and 7,168
unique FineWeb chunks: 8,229 sources, 35,875,486 unique source UTF-8 bytes and
8,359 current claims. The production index reported 8,359 lexical rows and
1,061 vectors, with both watermarks at 16,594. Indexing took 354.183 seconds.
All 8,359 current hash/revision/authority checks and all 1,061 exact retained
input/vector pairs passed before the first keyword witness failed.

That witness belonged to the `vectors` branch, but its query requested the
null branch. Ordinary scored hits require the exact scope. This was a harness
error; production retrieval was not changed. A separate oracle reconstructed
all 7,298 null-branch claims from the original FineWeb source and selected one
unique keyword witness per partition before querying.

The stopped store was reopened with the same configuration, provider port/model
and persisted index. Eight corrected keyword witnesses and eight vector self
hits passed without seed or index replay. Index generations, row counts and
watermarks remained unchanged. Keyword queries produced no embedding calls;
the eight vector queries produced exactly the expected 512 audit bytes. This
does not measure full ANN recall or prove every indexed body is searchable.

| Observation | Original import/index run | Corrected reopen |
| --- | ---: | ---: |
| Complete enforced daemon samples | 317 | 17 |
| Sampled daemon RSS maximum, bytes | 385,953,792 | 163,909,632 |
| Nearest-rank sampled RSS P99, bytes | 366,673,920 | 163,909,632 |
| Sampled OOM kills | 0 | 0 |
| Lookup process RSS high-water, bytes | 377,704,448 | 362,717,184 |

Daemon limits remained 512/640 MiB and service-total limit 2,048 MiB. Lookup and
controller had separately observed 512 MiB ceilings; they were not charged to
the daemon's service cgroup. The initial run still has a failed terminal and no
success result. Its earlier completed checks and the corrected reopen are
separate receipts, not one uninterrupted passing run.

Both runs use source `3b53674` / tree
`3bfbfa1095c9d8988744f76800e1dfcfee3a6e65`, but different release artifacts:
the import/index binary SHA-256 is
`c8caa9b73e1febe55e0ee076af8a5a80e2abdbbd812f12e847b96982ee901c9e`
(`cargo build --locked --release --bin akasha-otsuge --example corpus_seed`);
the reopen binary is
`4c29824abf0e2466fae4e0c32e2f1a37d5635bd5621244f6ff1ab2fec4f8ac1e`
(`cargo build --workspace --release --locked`). The precise hash-difference
cause remains unclassified; no reproducible-build or identical-binary claim is
made. The seeder SHA-256 is
`6622c1a5825170aa1fb42638a6f48e7e22bc4403a7a6111e54785e9430f63fb0`.

All children and owned cgroups ended. After hashing all 755 remaining native
files and checking consumers, the exact 152,547,328 allocated-byte store and its
owned socket directory were removed. Earlier disk samples are retained and may
be larger because native pruning continues during the run. Shared corpus,
environment, target and caches remain intact.

The final canonical gate, independent review and publication are separate from
this archive. Full 50 GB import/coverage, one million production vectors,
100,000 changes, 100-client load and the unchanged recovery criteria remain open.
