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

## Offline import

Select a fresh destination data directory in a separate config, then run:

```sh
akasha-otsuge --config /path/fresh-config.toml archive import /path/new-archive
```

The importer validates supported versions, bounded JSON, both complete file
hashes and lengths, contiguous blob ranges, exact content hashes, request hashes,
receipt references, unique requests/sequences and complete predecessor chains.
It preserves original commit numbers, timestamps, all four record kinds and
every revision, including retired content. It does not normalize UTF-8 or
regenerate content, vectors, timestamps or approvals. Hashes establish internal
consistency, not the trustworthiness of a supplied archive.

The manifest is limited to 64 KiB; the existing batch, blob and line limits above
also apply to input. Import reads one batch at a time and uses the native store
for staging and sequence-ordered history reconstruction. It does not sort the
complete archive in memory or make another copy of `blobs.bin`.

The destination's private `.archive-import` directory is bound to the exact
manifest hash before native staging begins. An exclusive import lock prevents
concurrent importers. Original receipts/revisions are committed atomically in
the stage; heads are reconstructed in original commit order. After validation
and native close, the completed directory is published as `authority` with a
no-replacement rename and parent sync. A concurrent daemon or existing store
cannot be overwritten. Unrecognized staging directories are preserved and
rejected. No application commit is added for the import itself.

Run the same command to resume a recognized stage after interruption. It
revalidates the input and saved records before resuming incomplete commits or
head reconstruction. Four real SIGKILL tests cover durable record staging,
head reconstruction, before publication and after publication; these are not
power-loss tests. Invalid archives remove only their recognized unpublished
stage after successful close. I/O failures retain that stage for inspection or
retry. If even the initial ownership marker is incomplete, import refuses it;
inspect that exact directory instead of deleting arbitrary staging paths.

An identical completed archive can be imported again without changing later
local commits. The entire archive and original stored revisions are checked
again. A different archive or unrelated destination returns `IMPORT_CONFLICT`;
merging incremental archives into an existing authority store is not implemented
in this leaf. Successful output describes the imported archive, not the current
sequence of an existing destination. A later close, publication-sync or output
failure still returns a nonzero status; rerun the same input to reconcile it.

## Starting the imported store

Use a local destination config and start the daemon after import completes:

```sh
akasha-otsuge --config /path/fresh-config.toml daemon start
```

Startup appends fresh local configuration and a local-owner read grant. It picks
a configuration generation above every archived generation, empties the runtime
job/incident queues, starts notification projection after the import boundary,
and selects a fresh search generation with zero progress. These initialization
commits are separate from the original imported commits. An interrupted startup
resumes initialization; completed initialization is not repeated on restart.
Search rebuilding and configured model calls require subsequent authorized work.

Historical configurations, grants, jobs, tokens and notification deliveries remain
readable records. They do not become current execution authority. Imported job
IDs return `IMPORTED_RUNTIME_AUTHORITY` on admission or claim, even if their
mutable records were later updated. Use a new local job ID and a freshly
validated plan. Imported invocation IDs and registration tokens return
`BINDING_UNAVAILABLE` on active use; register the host locally and use a new
invocation. Pi session and Claude child registration identities receive a local
import namespace, retaining the original immutable records.

Acknowledging an old notification does not make it eligible for delivery. A new
authorized review/incident change can create a new notification generation with
fresh delivery state. Original delivery history remains available in historical
revisions; old external message IDs are not reused for a new destination.

Memory authority, source spans, scopes, revisions, commit clocks and recall
outputs retain their original values. Current read grants still control access,
including history and receipt replay. Import does not rewrite another user's
scope or manufacture approval, and it does not remap a different operating-system
UID automatically. The integration fixture verifies the same local owner.

Controlled tests cover two real daemon starts without an external notifier call,
fresh capture and notification work, state/history/receipt comparisons and
identical archive replay after local changes. Four additional SIGKILL points
cover configuration, queue, index and grant initialization. A controlled Pi
fixture re-registers the same imported session with a fresh token and preserves
the new binding across restart while rejecting the old token. This is not
actual provider authentication. SIGKILL is not
power-loss or real ENOSPC evidence. Actual mempal conversion, incremental
application of different archives and full migration acceptance remain in #25.
