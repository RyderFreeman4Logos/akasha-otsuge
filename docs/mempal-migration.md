# Read-only mempal conversion

The first conversion stage preserves legacy SQL rows as inert evidence in a
neutral archive. Memory/source projection, repeated conversion and interrupted
conversion recovery are still under development in issue #25.

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

The archive destination must be new. Invalid source/version/structure is refused
before output creation. A later write or source-stability failure may leave a
private incomplete output without a final manifest; it is not a completed archive.
Current conversion does not resume such output. A completed archive uses the
existing neutral importer and its repeat/import-recovery contract.

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

The structural SQL fixture contains no real rows. Controlled tests are separate
from the private real-source audit and from future real target-equivalence proof.
