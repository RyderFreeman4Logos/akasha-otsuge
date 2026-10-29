# Native Pi 1.4.0 session trees

These exact JSONL files were emitted by the installed native `SessionManager`
on 2026-10-29. Content is synthetic; there was no model request or extension
Caller/Runner acceptance. `manifest.json` binds the package, generator, native
implementation, IDs, paths and file hashes. Paths refer to the removed private
SSD fixture, not a user session. No file content was rewritten after generation.

The fixture covers a branch whose leaf is not the final file entry, another root,
forked copied ancestry, later parent events, append-only context replacement and
omission, original public messages/tool results and native compaction. Synthetic
`PRIVATE_*` sentinels check that system configuration, thinking/signatures and
tool details never enter captured output. The U+2028 in the original user text
checks LF-delimited parsing.

Reproduce with Node >=22.19.0 and the normal mise-installed
`@earendil-works/pi-coding-agent@1.4.0`:

```sh
node generate.mjs /absolute/path/to/installed/pi-coding-agent /private/empty/scratch
```

The generator writes `scratch/fixtures/` plus native sessions/workspace. Keep the
three JSONL files and their generated manifest, then remove that owned scratch
after the process exits. New IDs/timestamps are expected. The checked-in manifest
also records SHA-256 for `generate.mjs` and the installed
`dist/core/session-manager.js`.

Run `cargo test --test pi capture::` for tree, original-preservation, durable
resume/rotation/replay and fail-closed format/identity regressions. The adapter
preserves fork `parentSession` as provenance and reads only the copied fork path;
it does not infer authority to open the parent path. Missing parent entries remain
explicit lineage gaps. Current model-context edits do not rewrite raw history.
