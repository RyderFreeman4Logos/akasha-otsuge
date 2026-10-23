# Public storage experiment input

`sources.json` pins the available source shards and upstream SHA-256 values.
It is an inventory, not proof of download, indexing, 50 GB coverage or acceptance.
Prepared and database files stay outside Git in an owned temporary directory.

- [Qdrant's one-million-vector dataset](https://huggingface.co/datasets/Qdrant/dbpedia-entities-openai3-text-embedding-3-large-1536-1M):
  revision `4a9731217921bc476a0f03544f11f22ae4903fa5`; precomputed February 2024
  `text-embedding-3-large` vectors with 1536 dimensions, based on title plus text.
  The publisher declares MIT; the underlying
  [BEIR DBpedia text](https://huggingface.co/datasets/BeIR/dbpedia-entity) declares
  CC-BY-SA-4.0. Preserve both attributions. No paid inference or user credentials
  are used, and the experiment does not change the product's default model.
- [FineWeb-Edu](https://huggingface.co/datasets/HuggingFaceFW/fineweb-edu): revision
  `87f09149ef4734204d70ed1d046ddc9ca3f2b8f9`, `sample/100BT` pool. The dataset
  declares ODC-BY; original text rights remain with their owners. The content is
  local benchmark input, not relicensed or distributed as part of this repository.

Public corpora are untrusted data. They are never executed, loaded as model
instructions, or sent to private services. Source hashes are verified before
Parquet decoding; decoding uses a private environment with `pyarrow==26.0.0`.
No user sessions or preexisting memory databases are read.

To prepare an already downloaded, checksum-pinned vector shard:

```sh
uv venv .spike-work/data-env
uv pip install --python .spike-work/data-env/bin/python pyarrow==26.0.0
.spike-work/data-env/bin/python spikes/data/prepare_vectors.py \
  /owned/path/vectors.parquet /owned/path/new-vectors.jsonl
```

The converter checks dimension, finiteness and nonzero vectors, counts distinct
float32 vectors and content hashes, and assigns deterministic partitions from
SHA-256 of each source ID. Its manifest records actual bytes, counts, source and
prepared hashes. A shard manifest explicitly says indexes and the full acceptance
have not run. FineWeb conversion, global corpus deduplication, full lexical/vector
coverage and the historical oracle are still required before the scale gate.

Downloads use immutable `resolve/REVISION/PATH` URLs from this inventory, verify
size and SHA-256 before renaming a `.part` file, and reserve at least 30 GiB of
filesystem free space. Never count compressed Parquet bytes as original UTF-8
content bytes. The first diagnostic shard contains 38,462 vectors, not one million.
# Resumable downloads

`python3 spikes/data/download.py OWNED_DIRECTORY 16` verifies/downloads all 26
vector shards and the first 16 text shards using four workers. The directory
must have an `OWNER` marker. Partial transfers are resumed; final names are
published only after size and SHA-256 validation. Existing final files are
revalidated. The command reserves 30 GiB of free space. It does not count
downloaded bytes as eligible content or assert that 16 shards pass acceptance.

Downloads keep an fsynced `download-state.json`, per-file attempt receipts and
`download-progress.json` in the owned corpus directory. Resume a recorded run
with `download.py DIRECTORY TEXT_SHARD_COUNT --resume`; every completed file is
revalidated and interrupted GETs resume their byte offset. `download.toml` defines
bounded retries for transient transport errors, HTTP 429 and 5xx; each file records
the policy hash. A failed file is recorded while other files finish. Partial and
failed receipts are retained. Verified downloads are preparation, not indexed
benchmark coverage. ETA remains unavailable across unequal shard sizes.

## Reproducible full-corpus stream

`prepare_corpus.py scan DIRECTORY VECTOR_SHARDS TEXT_SHARDS NEW_MANIFEST` checks
source hashes, deduplicates content and counts real UTF-8 bytes and float32 vectors.
Long text is split on UTF-8 boundaries into at most 64 KiB records. Added title/text
separators are excluded from eligible bytes. All vectors precede text shards; text
stops once eligible content reaches 50,000,000,000 bytes. Smaller input produces an
explicitly diagnostic manifest. Synthetic revision-oracle text never contributes.

`prepare_corpus.py stream DIRECTORY MANIFEST` reproduces the same stream without
retaining another large JSONL file. It rejects code/environment drift before
output and verifies the complete statistics and stream hash at EOF. Consumers
must reject an import if the producer fails; receiving a prefix is not acceptance.
Run these commands with the pinned private PyArrow environment. Corpus preparation
memory is separate from measured database RSS. Neither command claims indexing.

`scan ... --wait-for-downloads` overlaps preparation with the managed downloader.
Only fully published source files are consumed. A missing source waits only while
the recorded downloader PID/start identity is live; a terminal file failure or
changed process identity fails the preparation. Per-shard progress is fsynced next
to the requested manifest as `.progress.json`. Incomplete preparation can be
restarted with a new manifest path: it rereads sources and does not claim saved
hash state or exactly-once execution. No database is modified by preparation.
