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

`native-root-resume-compact.jsonl` records one native session started, resumed,
then manually compacted against the same local SSE fixture. It preserves native
event UUIDs, parent links and compaction metadata. Native attachments keep their
type and rendered shape with synthetic text; absent rendered bodies stay absent.
API request metadata keeps only its type, session and version. Native summary
text and owned paths were replaced.
The complete source transcript has SHA-256
`4d65441bf974ca19bb5054258f8dd81ca4be97a8b5079c3c7083df081607a65e`;
the sanitized fixture has SHA-256
`3a55296106f37c5d91146c17e6ab32c2056aa7c12daef850e0bd4c9353eb091e`.
Capture follows the compaction boundary's native `logicalParentUuid` back to
the first retained ancestor, preserving original references and marking summary
messages as session information. This range describes captured ancestry, not
the model's exact input window. Missing ancestors, cycles or more than 65,536
ancestors leave the frame pending; no incomplete range is declared complete.
The regression also checks missing-ancestor refusal and retained summary flags.
Unrendered attachments retain their UUID and parent chain with no visible body.
The fixture was corrected after a real native `session_context` attachment
exposed that sanitization had supplied rendered text absent from the source.

`native-child.jsonl` records a native `Agent` child calling a fixture tool. It
retains all18 source records, native UUID/parent links, agent identity and
sidechain flags. Attachments keep their rendered shape with synthetic text;
tool results replace diagnostic paths with a marker and API metadata keeps only
identity/version fields. Source SHA-256:
`e20f6bae42d38efdcfa6b630b93031de0436dc6b982c36473c7811873be8cd14`;
fixture SHA-256:
`688b49ca9ea70296c6b98a2db64153d7ea8153a9eccc8b5e8c16c0eed038d3bd`.
The ordinary child has13 captured events with no inherited parent originals.
Its runtime parent registration remains distinct from transcript ancestry.

`native-fork.jsonl` is the41-record native CLI fork of the recorded root before
its later compaction. It retains25 native UUID-bearing records under the fork's
new session ID. The same sanitization rules replace attachment text and tool
result diagnostics; copied originals and parent links remain present. Source
SHA-256 `86d82c82b7a257db0e4ff19f1e45de3cca082b08d9e87e7158939bdb62a3a34f`;
fixture SHA-256
`2cf9bc8a37a4331ff89d69af32e5beac13b136eaa1d5e8350ceddcc1536e649e`.
Storage replay verifies retained startup/resume originals and isolation from
the parent's later compaction. The native harness also verifies that later
parent turns and fork inputs stay separate. SessionStart reports `fork` but
does not provide its parent session ID; the adapter leaves that ID unknown.

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

The Caller check uses the production native hook and actual daemon/Storage,
checking startup/resume/fork/child identity and completed durable backfill. Its
fixture-only tool returns a marker after binding; it does not implement consult
research or establish authenticated model acceptance. It removes its owned
scratch directory and processes on completion:

```sh
cargo build --locked
python3 scripts/check-claude-caller.py /absolute/path/to/claude \
  /absolute/path/to/akasha-otsuge \
  /absolute/path/to/new-ssd-scratch-directory
```
