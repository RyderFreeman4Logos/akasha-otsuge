# Neutral archive v1

`archive export` copies the complete application commit history into JSON and
original UTF-8 bytes. Reading this format needs neither SurrealKV nor LanceDB.
It is separate from the native database checkpoint API and from `memory export`,
which displays one memory.

Stop the daemon before exporting. Use the same config/profile as the stopped
store and a new destination whose parent directory already exists:

```sh
akasha-otsuge --config /path/config.toml daemon stop
akasha-otsuge --config /path/config.toml archive export /path/new-archive
akasha-otsuge --config /path/config.toml daemon start
```

The native database lock prevents export while another process owns the store.
Export requires an existing store, rejects destinations inside or above its
authority directory, and never overwrites an existing destination. `--socket`
does not select an offline store and is rejected for this command. Export adds
no application commits; opening the native engine is not a promise that its
physical files remain byte-identical.

The directory is private (`0700`) and files are private (`0600`). It contains all
saved original content and historical operational records, so treat it with the
same care as the source database. No external model or agent is invoked.

## Files and identities

There are exactly three completed files, independent of record count:

| File | Contract |
| --- | --- |
| `manifest.json` | `archive_version=1`, `record_format_version=2`, durable `commit_seq`, commit/revision counts, and each data file's byte length and SHA-256. |
| `commits.jsonl` | One JSON object plus LF per commit, in **request-key order**, not commit order. Each object contains the original `receipt`, `recorded_at_ms`, and `revisions`. |
| `blobs.bin` | Concatenated exact UTF-8 revision contents, without separators or normalization. Byte offsets refer to this file. |

A revision entry has `key` (`kind`, `id`), `commit_seq`, nullable `previous`,
`content_sha256`, `blob_offset` and `blob_bytes`. Ranges are contiguous in file
order. Empty content has a zero-length range. NUL, newlines and multibyte Unicode
remain ordinary original bytes. Identical content is retained at each revision;
hash equality does not merge different record identities.

`previous` references an earlier revision of the same key. The receipt retains
its request ID/hash, commit sequence, complete revision references, configuration
generation and policy revision. The writer checks those references against the
durable outbox and revisions and reconstructs the canonical request hash. The
stored commit clock is copied; original event timestamps inside content are not
substituted for it.

Record content keeps the application's existing logical schema: source spans,
scope, authority, lifecycle, approvals, corrections, recall outputs and other
relations remain in their original JSON/text. Readers must retain each record's
kind and ID and must not infer human approval from an arbitrary text field.
The archive does not contain physical SST/WAL files or derived search indexes.
Saved jobs, host bindings and configuration are historical data; parsing an
archive must not execute them or activate their paths, credentials or policies.

## Bounds and completion

Export streams one committed batch at a time. Existing storage limits apply:
64 changes per batch, 128 KiB content per revision, 1 MiB total content per batch.
Each JSONL line is at most 1 MiB including LF. Working memory does not grow
with the number of commits; a manifest contains only the two data-file entries.

Both data files are flushed and synced before `manifest.tmp` is synced and
renamed to `manifest.json`; the directory and its parent are then synced. A
successful command prints the manifest in the usual `schema_version/data`
envelope. Any I/O/close/output failure returns a nonzero exit status. The hashes
detect accidental corruption; they are not signatures or proof of provenance.

An interrupted or failed export may leave a private incomplete destination.
Missing, malformed or mismatched manifests must never be accepted. Keep the
source, inspect only the exact failed destination, and use a fresh destination
for a new attempt. A valid manifest left after a later close/stdout failure does
not turn that command's nonzero status into success.

This first MVP-025 leaf supplies export only. Import, a validating reader, mempal
conversion, duplicate/incremental application, interruption recovery and complete
round-trip acceptance remain pending; issue #25 stays open.
