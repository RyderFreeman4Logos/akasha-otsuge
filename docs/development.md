# Development

GitHub CI caches Cargo registry/git sources and `target` by OS, architecture,
toolchain and manifest/lock hashes. It always runs the full check command,
including on a cache hit. Cache misses remain ordinary clean builds; credentials
and user configuration are excluded. The pinned official [cache action](https://github.com/actions/cache/tree/55cc8345863c7cc4c66a329aec7e433d2d1c52a9)
saves only successful runs. No paid cache-capacity setting is changed.

Linux is the MVP platform. Install Rust using rustup, Python 3.11 or newer, Node.js 18 or newer,
a C/C++ toolchain, the Protocol Buffers compiler (`protoc`) and its standard
`.proto` definitions. Lance's build scripts require both; neither is a runtime
dependency. On Debian/Ubuntu:

```sh
sudo apt-get install build-essential pkg-config protobuf-compiler libprotobuf-dev nodejs
```

The local acceptance environment uses `protoc` 3.21.12. CI installs the distribution
package and records its version before building.
Node runs the native hook's isolated JavaScript regressions; it is not a product runtime dependency.
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

The capture component smoke reuses the debug example already compiled by
[`cargo test`](https://doc.rust-lang.org/cargo/commands/cargo-test.html#target-selection).
The installed product and daemon acceptance still use the release artifact.
This avoids compiling a second release dependency graph with test-only Tokio
features merely to run the component smoke.

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
