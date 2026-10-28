# Native Claude Code 2.1.311 fixtures

Recorded on Linux on 2026-10-28 with the official native executable SHA-256
`86dc1fcc69498159eaf15f446613e847010f9036bfbbdffd4c49350be8489ada`.

`authentication-failed.json` is the native assistant error from an isolated,
unauthenticated print invocation. Only `session_id` was replaced. The complete
original stdout has SHA-256
`555580260197dd05e1f5af9e34c2e46f81212127630f358a0138e45ecba2adf7`.

`api-retry.json` is the first retry event from a native invocation against a
closed loopback port. Only `session_id` was replaced. Its complete source stdout
has SHA-256 `c026cf4c43fec64260693bbd77ca638120062540239c08cb3f8e34acb280105b`.

`native-tool-roundtrip.jsonl` records the native CLI calling this adapter's actual
plugin and read bridge against real `CapturedContext` fixture storage. A local
Anthropic SSE fixture requested `context_list`, then `context_read` using the
returned handle, then a final answer. It verified that the initial request did
not contain the original and that the original appeared only after the read.
These are protocol checks; no authenticated model was evaluated.

The recorded session ID, scratch path and opaque read handle were replaced with
fixture values. All text is synthetic. The complete original stdout has SHA-256
`62b510db7eb7ac6344851647f9410496677a8acd2d3c516c3fc9a254bc23b92b`.
The native first attempt failed because the adapter rejected `user/tool_result`;
after adding paired tool-result handling, the unchanged three-request fixture
passed. The normal suite replays these native events and mutates pairing, child
identity and completion boundaries to verify refusal.

To repeat the local protocol check, compile the test executable and use the path
Cargo prints. The last argument must be a new artifact directory on writable SSD
storage. The script uses isolated HOME/config, a synthetic key, loopback HTTP and
a PATH containing no other agent CLI. It retains bounded request/stream evidence
in that directory; remove the directory after archiving the needed receipts.

```sh
cargo test --locked --test adapter_context --no-run
python3 scripts/check-claude-protocol.py /absolute/path/to/claude \
  /absolute/path/to/adapter_context-<cargo-reported-hash> \
  /absolute/path/to/new-ssd-artifact-directory
```

Native API references: [CLI options](https://code.claude.com/docs/en/cli-reference),
[plugin modules](https://code.claude.com/docs/en/plugins/mods/api), and
[Anthropic SSE](https://platform.claude.com/docs/en/build-with-claude/streaming).
