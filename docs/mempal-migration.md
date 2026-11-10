# Read-only mempal conversion

The converter preserves legacy SQL rows as inert evidence in a neutral archive
and projects original text and unverified memory candidates. Repeated and
interrupted conversion reuse the same private output directory. A real supported
snapshot passed all-row equivalence, native roundtrip and isolated daemon reads.
Different-source incremental application remains under development in issue #25.

Prepare a consistent, stopped SQLite snapshot independently. Do not copy only
the main file of a live WAL database. The converter refuses a snapshot with an
adjacent WAL, SHM or journal file, opens the retained file descriptor read-only
with `immutable=1`, and checks its identity and complete SHA-256 again before
publishing the archive manifest. It never invokes mempal, migrations, model
providers, queued work or extension loading.

```sh
akasha-otsuge --config /path/destination.toml \
  migrate mempal /path/private-snapshot.db /path/new-archive
akasha-otsuge --config /path/destination.toml archive import /path/new-archive
```

The archive destination must be new or an owned private conversion directory.
The source must be outside that directory, including when its parent is reached
through a symlink. Writable output handles also reject the retained source inode
before truncation or append, so moving that inode after reader admission cannot
turn it into a staging file. Refusal preserves source bytes.
Identity and manifest publication write a new unnamed temporary file, sync it,
and create the final name without replacing an existing file. Existing legacy
`.tmp` members are validated as private files and left untouched. The destination
filesystem must support Linux `O_TMPFILE` and hard links; unsupported operations
return an I/O error without a replacing fallback. This uses the Linux
[unnamed-file publication](https://man7.org/linux/man-pages/man2/open.2.html) and
[non-replacing link](https://man7.org/linux/man-pages/man2/link.2.html) contracts.
Invalid source/version/structure is refused before output creation. A later write
or source-stability failure may leave private incomplete output without a final
manifest; it is not a completed archive. Repeat the same command to reconcile it.
The converter pins the source hash, mapping version, importing UID and first
observation time in `conversion.json`, holds one output lock, and regenerates the
deterministic stream. Existing bytes must match exactly before missing suffixes
can be appended. It does not duplicate existing records or copy the input database.

Completed output is checked in full without appending or changing its observation
time. A different source or conflicting bytes returns `MEMPAL_CONVERSION_CONFLICT`;
a held writer lock returns `MEMPAL_CONVERSION_BUSY`. Invalid private file ownership,
permissions and links are refused. Incomplete reconciliation can append verified
suffixes before a later failure, but cannot publish a manifest until the complete
stream and unchanged source pass. There is no promise of constant-time resume:
source validation and deterministic prefix comparison run again. A completed
archive uses the neutral importer's separate repeat/import-recovery contract.

Support is restricted to upstream schema **21** and fork extension **28**, with
the table/column layout mapped from source commit
`4593a5c5e6a9ea1140256cd42fa5838511a24e14` and checked against a real snapshot.
Matching schema axes do not establish the installed mempal binary's source
commit. All 28 core ordinary tables are required; the auxiliary
`historical_rejudge_work_items` table is optional. Unknown tables, views, missing
columns and changed column types are refused. Known derived FTS/vector/internal
tables are listed as excluded; no vector extension is needed. SQLite quick_check
does not validate vec0 semantic contents.

All retained tables use typed JSON rows in original SQLite rowid order. NULL,
integer, IEEE754 real bits, exact UTF-8 text and binary bytes remain distinct.
Original IDs, scope/time fields, provenance claims, deletion states, relations,
audit events and pending payloads retain their values. Binary bytes use hex;
reals use 16 hex digits of their IEEE754 bits. A table descriptor records column
order, row/block counts, byte length and SHA-256 of its concatenated JSONL data.
UTF-8 blocks are at most 32 KiB before JSON escaping; commits contain at most 64
records and 1 MiB. The SQLite reader caps one SQL value/row at 16 MiB and refuses
oversized input instead of truncating it.

Rows are evidence under `mempal/SOURCE_SHA256/table/TABLE`; they never occupy job,
grant, configuration or native-binding keys. The migration record retains source
identity, supported version axes, per-table descriptors and derived exclusions.
New archive commit clocks record conversion observation time, not fabricated
legacy commit times. This stage does not manufacture memory authority, approval,
original documents or historical revisions that the old database never saved.

Mapping version 2 also creates native source records for drawer content, knowledge
card statements/content and conversation turns. Text is split at UTF-8 boundaries
into at most 16 KiB chunks; every candidate cites the exact source chunk hash,
range and revision. Source, candidate and pending review share one atomic archive
commit even at batch boundaries. Original row IDs and all other fields remain in
the typed table evidence; projected IDs include the snapshot hash, table, rowid
and chunk number.

Drawer claims and card statements become manual-review candidates, never active
or human-approved memories. Deleted, compacted, retired, superseded or demoted
rows retain sources without creating candidates. Nonempty scope constraints or
invalid validity intervals also retain sources only, counted in the migration
report. Parseable validity bounds remain on candidates, including expired ones.
Drawer kinds map decision/profile_trait/skill/foresight to
decision/preference/procedure/hypothesis; other supported kinds and cards map to
fact. Conversation turns create sources without manufacturing memory claims.

Valid drawer project IDs and conversation project/branch fields are preserved.
An explicit global domain/global anchor permits a global drawer/card; missing or
unrepresentable project scopes otherwise use an isolated namespace. Worktree
anchors add a restrictive branch namespace. Conversation session scope includes
tool, Hermes profile and native session ID, so shared IDs do not merge profiles.
All projected records use the importing user's UID and unverified provenance.
Legacy role/provenance strings never authenticate a human or grant access.

RFC3339 and SQLite UTC timestamps are recognized; finite conversation epoch
seconds are converted to milliseconds. Unrecognized event dates remain unknown
and are counted, while their original fields remain intact. Conversion observation
time stays distinct from legacy event time. Retained relation/audit rows do not
establish absent original documents, missing historical versions or approvals.

The structural SQL fixture contains no real rows and retains the source
[MIT notice](licenses/mempal-MIT.txt). Controlled fixtures and real private-data
acceptance remain separate in the [evidence archive](benchmarks/results/mvp025-mempal-reader/README.md).
The actual run retained 3,525,645 rows in 29 tables, with matching independent typed
row digests and an unchanged source file. Its 5,984,986,114-byte archive contains
27,511 commits and 1,687,022 revisions. Two fresh native imports, one duplicate
import and two complete exports preserved every archive byte, receipt and clock.
Two isolated daemon starts then read three distinct candidates' original chunks,
historical records and pending reviews; none became active and no legacy jobs ran.
These results do not establish the separate resource/load or model-quality gates.
