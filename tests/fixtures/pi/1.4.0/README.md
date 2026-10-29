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

## Native CLI protocol and lifecycle qualification

`check_native_runner.py` and `check_native_caller.py` use the actual Pi1.4.0 CLI,
the compiled Akasha executable and an isolated, synthetic loopback SSE provider.
They create and remove their own private profile, extension installation, daemon
data and native sessions. They do not read the user's native authentication.
Caller lifecycle commands come from the acceptance-only `lifecycle.ts` extension;
that extension is not installed in product profiles. The compact acceptance
receipt and three unmodified native output files are in `rpc-controlled/`.
The files contain only synthetic content and paths to already removed scratch.

Build with `cargo build --locked` and `cargo test --test pi --no-run --locked`,
then use the test executable printed by the latter command:

```sh
export AKASHA_BINARY=/absolute/path/to/target/debug/akasha-otsuge
export AKASHA_PI_BINARY=/absolute/path/to/pi-1.4.0
export AKASHA_PI_TEST=/absolute/path/to/target/debug/deps/pi-TEST_HASH
export AKASHA_NATIVE_PATH=/directory/containing/node:/usr/bin:/bin
export TMPDIR=/private/writable/ssd/tmp
export PYTHONDONTWRITEBYTECODE=1
python3 tests/fixtures/pi/1.4.0/check_native_runner.py /new/runner-output
python3 tests/fixtures/pi/1.4.0/check_native_caller.py /new/caller-output
```

Output directories must not exist. The scripts require128MiB of free scratch
space and reject a PATH containing Claude, Codex or Hermes. New IDs, paths and
timestamps are expected. The retained receipt binds source/binary/file hashes,
five Runner cases, six Caller bindings,19 completed jobs and cleanup. The retry
case causes one real local HTTP503 before native recovery. The hostile-tool case
requires `TOOL_DENIED` and verifies its outside marker was never created.
Set `AKASHA_NATIVE_API=openai-responses` to exercise native compound tool-call IDs;
the default is `openai-completions`. Both use the native provider parser.
`protocol-controlled/manifest.json` pins the corrected bridge, fixture sources,
all six protocol/lifecycle runs, failed pre-fix evidence and cleanup. Its five
unchanged native files come from the Responses RPC and TUI runs.

For controlling-terminal coverage, supply a directory containing executable `fd`
and `rg`, then run `check_native_tui.py` with the same environment:

```sh
export AKASHA_TUI_TOOLS=/directory/containing/fd-and-rg
python3 tests/fixtures/pi/1.4.0/check_native_tui.py /new/tui-output
```

The script gives each Pi process a foreground controlling PTY and waits for its
native `session_start` event. The tools prevent Pi from downloading them during
startup. It checks two overlapping sessions, fresh-process resume, native compact,
four bindings,11 completed jobs and zero-exit cleanup. A first diagnostic failed
when startup-triggered fd download raced and input arrived before readiness;
that failed receipt remains separate from the successful prepared-tool runs.
Authenticated provider acceptance remains a separate requirement.
