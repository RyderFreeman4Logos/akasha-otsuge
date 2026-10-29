# Codex0.164.0 acceptance

[acceptance.json](acceptance.json) contains the sanitized native receipts and
hashes. It separates authenticated model runs from controlled Responses SSE
fixtures. Both use the actual pinned Codex executable. It is runtime evidence;
the final source gate, independent review, CI and merge have separate receipts.

- Existing ChatGPT authentication: the standalone product Runner called its
  scoped context tool, returned the requested structured answer, and exited zero.
- Two actual TUIs shared one workspace and native home. Both had controlling
  terminals, overlapped, returned distinct exact session/call fences, contained
  only their own synthetic marker, and exited zero. Other agent CLIs were absent
  from PATH. The native database was initialized once before simultaneous startup.
- Native forced attempts to write, use tool networking, read outside context,
  apply a patch or invoke an unregistered MCP tool produced unsupported-tool
  results without the sentinel effect. An unissued context handle produced
  `READ_DENIED`. The model's actual tool inventory contained exactly four context
  tools. The controlled endpoint supplies hostile responses; prompts are not the
  enforcement boundary.
- Versioned native history fixtures cover original public messages/tools,
  compaction, fork cutoffs and separate-process resume. Product tests additionally
  cover interrupted backfill, permission revocation, duplicate capture and gaps.

To repeat the authenticated standalone Runner against a configured native home:

```sh
AKASHA_CODEX_BINARY=/absolute/path/to/codex \
AKASHA_CODEX_HOME=/absolute/path/to/native-home \
AKASHA_CODEX_ACCEPTANCE_SECONDS=180 \
cargo test --locked --test codex \
  runner::installed_native_runner_roundtrip -- --ignored --exact
```

This explicitly spends one model task using that native home's credentials.
Ordinary `cargo test` skips it. Do not run the ignored hostile-handle test against
a normal model: it requires a controlled endpoint that emits the specified
unissued handle. The committed fake-host fault suite runs without authentication.

For Caller acceptance, install the opt-in launcher as described in
[the adapter guide](../../adapters.md), review its hooks in Codex, then open two
terminals in the same workspace using that launcher. In each ask for one
`exec_command` invocation of `akasha-otsuge consult "distinct-marker"` and compare
the returned `caller.session`, `tool_call_id`, `turn_id` and call cursor with its
own native rollout. The present command returns a binding diagnostic; the recall
workflow is MVP-016. Do not interpret missing child proof as the root session.

All native scratch, fixture credentials and owned processes were removed after
each run. The successful two-TUI fixture allocated 5,115,904 bytes excluding
symlinks and duplicate inodes. Earlier apparent-size sums followed native arg0
links repeatedly and are not a measurement of newly allocated storage. Failed
attempts remain recorded, including native first-start database contention and
PTY input submission timing; the successful fixture did not alter user security
settings or global hook trust.

Pinned contracts: [app-server](https://learn.chatgpt.com/docs/app-server),
[hooks](https://learn.chatgpt.com/docs/hooks),
[configuration schema](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/config.schema.json),
[native tool policy](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/tools/spec_plan.rs),
[nested Code Mode identity](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/tools/code_mode/delegate.rs).
