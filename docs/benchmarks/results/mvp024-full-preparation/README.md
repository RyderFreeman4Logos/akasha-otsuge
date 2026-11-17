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

Commit `af8eb1f834cd2ef21fe0bdd65f6a1d3e9cd4a69d` applies that reader setting in `spikes/data/prepare_corpus.py`.
All four existing preparation tests pass, including real Parquet deduplication,
UTF-8 chunk boundaries and vector validation. The source change requires a new
manifest. Its committed-source scan is recorded below; production import remains pending.

`evidence.zip` and `manifest.json` retain both complete run receipts, the original
failed resource result, runtime override, exact source snapshots, focused tests
and a parent aggregation path error. No raw Parquet or native database is copied.

## Committed source validation and remaining admission failure

The committed preparer reproduced the entire original corpus statistics and byte
stream hash. The new manifest SHA-256 is
`88f767e86e790b0a1714b229bc0b3d047724ee7f2479596624c9daffacb9fd5d`;
its only difference from the original manifest is `code_sha256.prepare_corpus.py`.
GNU time peak RSS was 1,992,634,368 bytes, below the unchanged 2 GiB observed
preparer ceiling. A check-only wrapper examined each yielded record and claim;
it did not alter the reader, output records or stream order.

That wrapper found 13 records whose current production split creates an empty
claim. All 13 are 65,536-byte sources; the exact empty-span lengths have not yet
been measured. The complete census has 11,564,678 planned claims and 999,963
eligible original vector inputs. The overall run is **failed** for production
record admission, despite passing source reproduction and preparation memory.
There is no success result or production import. These records must be diagnosed
without silently filtering source bytes or weakening the production validator.

`source-validation.zip` contains 20 members, 23,920 bytes, SHA-256
`9260e6fa170836b21e8426ca153af762b7e8ef5f2501a9a9c7de9c40b81d215e`.
`source-validation-manifest.json` gives the hashes of the complete receipts,
new source manifest, wrapper, all rejection identities and original failed terminal.
