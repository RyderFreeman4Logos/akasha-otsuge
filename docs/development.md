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

The gate validates specification hashes and the issue map, checks negative
controls, and runs formatting, Clippy, component/process tests and a release build.
GitHub Actions runs the same gate in a clean Ubuntu 24.04 checkout. Daemon acceptance
installs that release into a temporary prefix, checks its hash, and exercises the
installed executable's IPC, configuration, jobs, history and receipts. A separate
kernel UID must pass an accessible control and be denied private paths/IPC.
Installing into the user's PATH remains a separate action.

There is one Cargo package in one workspace. `src/main.rs` parses CLI input and
calls versioned private IPC. The daemon invokes the shared Storage, ConfigManager,
Capture and Domain components; it does not provide placeholder domain results.
`tests/daemon.rs` exercises real executable and restart boundaries. [Model](providers.md) and [retrieval](retrieval.md) jobs share that daemon.
Host integration and full-scale product acceptance retain their own criteria.

`src/config/` contains the shared configuration schema, atomic generation manager
and monotonic deadline contracts. Its controlled consumers are tested in
`tests/config.rs`; see [configuration](configuration.md) for integration boundaries.

Version output uses Cargo's compiled package version. The immutable Git commit and
CI run identify the exact source; no changing wall-clock timestamp enters the binary.

Errors use one JSON line on stderr:

```json
{"schema_version":1,"error":{"code":"INVALID_ARGUMENT","message":"Use --help for supported arguments"}}
```

Exit 0 is success, 2 invalid command/argument, 3 daemon unavailable, 4 busy,
and 1 other failure. Invalid-command errors leave stdout empty and do not echo
untrusted arguments. See [the protocol](daemon.md). Unsupported features return an error;
they do not return placeholder success.

Implement one main Issue at a time, preserve its requirement/dependency mapping,
run the shared gate on the candidate commit, review the diff and record evidence
before merging and closing the Issue. Specification snapshots remain immutable.
See `docs/plan/issues-index.md` for the full required scope.
