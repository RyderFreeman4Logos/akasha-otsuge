# Development

Linux is the MVP platform. Install Rust using rustup and Python 3.11 or newer.
The checked-in toolchain pins Rust 1.99.1, verified against the official stable
channel manifest on 2026-10-22. No model credentials are required for this bootstrap.

```sh
cargo build --release --locked
target/release/akasha-otsuge --help
target/release/akasha-otsuge --version
bash scripts/check.sh
# Equivalent when just is installed:
just check
```

The gate validates the specification hashes and issue map, checks its negative
controls, runs Rust formatting, Clippy, process-level CLI tests and a release build.
GitHub Actions runs the same gate in a clean Ubuntu 24.04 checkout. The release
binary is tested in place; installing it into a user's PATH is a separate action.

There is one Cargo package in one workspace. `src/main.rs` currently contains only
help/version and error handling; `tests/cli.rs` invokes the actual executable.
Add modules when behavior requires them. Storage selection belongs to MVP-003;
no database or host integration is implied by this bootstrap.

`src/config/` contains the shared configuration schema, atomic generation manager
and monotonic deadline contracts. Its controlled consumers are tested in
`tests/config.rs`; see [configuration](configuration.md) for integration boundaries.

Version output uses Cargo's compiled package version. The immutable Git commit and
CI run identify the exact source; no changing wall-clock timestamp enters the binary.

Errors use one JSON line on stderr:

```json
{"schema_version":1,"error":{"code":"INVALID_ARGUMENT","message":"Use --help for supported arguments"}}
```

Exit 0 is success, 2 is an invalid command/argument, and 1 is an execution/I/O error.
Invalid-command errors leave stdout empty and do not echo untrusted arguments. Future error codes
follow the distinctions in `docs/spec/02.md`. Unsupported features return an error;
they do not return placeholder success.

Implement one main Issue at a time, preserve its requirement/dependency mapping,
run the shared gate on the candidate commit, review the diff and record evidence
before merging and closing the Issue. Specification snapshots remain immutable.
See `docs/plan/issues-index.md` for the full required scope.
