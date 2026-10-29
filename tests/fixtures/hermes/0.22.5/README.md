These are real canonical CLI exports of synthetic messages, produced with
Hermes `0.22.5+1914.gfdb04a0`, source commit
`fdb04a00c1626acb589552f5634a383e2b4beccc`. They are parser fixtures, not evidence
of authenticated model, Caller, Runner or TUI acceptance.

The fixture creates a temporary native `SessionDB`, appends five messages with
explicit `message_uid` values, and calls:

```sh
hermes sessions export OUTPUT --session-id SESSION --format jsonl
```

It then invokes native `archive_and_compact(..., tail_count=1)`, appends another
question and exports again. A separate `delegate_task` session records the
parent session ID. Each file contains exactly one complete native session.
The only output substitution replaces the disposable fixture root with
`/synthetic/fixture`. `manifest.json` records SHA-256 hashes and native row counts.
The private-reasoning and configuration sentinels are invented test strings.

The source canonical export includes inactive rows. Rows archived by compaction
have `compacted=1`; superseded carried copies and undone rows have both flags
zero. Clones preserve native UIDs and timestamps but receive new physical row
IDs. Parsing must retain originals, deduplicate carried messages, omit undone
rows and exclude private reasoning/configuration. The repeated-compaction unit
case derives a second native-shaped snapshot from these fixtures; it is not an
additional CLI export.

The collection used one disposable SSD profile with native dependency sharing,
the native lazy-install disable flag and `max_export_messages: 64`. The run
allocated 380,928 bytes across 14 files and removed its profile alias and scratch
after completion. No model was called and no user transcript was exported.
