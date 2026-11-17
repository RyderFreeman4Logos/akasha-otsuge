# Full corpus preparation memory

The original full scan at `1c6eed5` reproduced the retained manifest byte for byte:
11,428,831 deduplicated documents, 11,806,304 source rows and 50,000,002,783
eligible UTF-8 bytes. It hashed the 84,147,341,473-byte serialized stream without
writing that JSONL expansion. Source hashing consumed 41 shards from the maximum
42-shard selection; the text-byte target ends inside the final consumed shard.

The scan exited successfully, but its GNU time peak RSS was 3,829,080,064 bytes,
above the predeclared 2 GiB observed preparer ceiling. The overall run remains
failed. This is an offline preparation measurement, separate from daemon,
runner and service budgets; no kernel hard-limit enforcement is claimed.

One control changed only `ParquetFile(pre_buffer=False)` at runtime, retaining
the original frozen source files and manifest code identities. It reproduced
the complete original manifest exactly, with peak RSS 2,012,385,280 bytes below
the unchanged 2,147,483,648-byte ceiling. There was no cache eviction. These two
observations do not establish a general performance interval or worst-case bound.
The control is not mislabeled as execution of a new committed source.

The candidate applies that reader setting in `spikes/data/prepare_corpus.py`.
All four existing preparation tests pass, including real Parquet deduplication,
UTF-8 chunk boundaries and vector validation. The source change requires a new
manifest; its committed-source scan and full production import are still pending.

`evidence.zip` and `manifest.json` retain both complete run receipts, the original
failed resource result, runtime override, exact source snapshots, focused tests
and a parent aggregation path error. No raw Parquet or native database is copied.
