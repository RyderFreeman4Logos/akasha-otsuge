# Claude Code 2.1.322 on Linux

The official x86_64 executable reported `2.1.322 (Claude Code)`. The runs used a private HOME, workspace, network namespace and loopback SSE fixture. The receipt binds the executable and native output streams by SHA-256. [Native compatibility receipt](native-compatibility.json)

The installed Caller plugin captured a headless turn, two concurrent sessions and a controlling-terminal session. The terminal resumed its prior session, compacted it and captured the next turn. Six durable capture checkpoints identify the original native transcript bytes. A separate test of the native Caller bridge observed five bindings and 13 backfill jobs across startup, resume, fork, subsequent parent activity and an Agent child. Its fixture tool exercised binding. The native Runner made three requests, listed and read context through the real bridge, paired both tool results and delivered its final response.

In the four-turn branch probe, the fork received the parent’s earlier two inputs and its own new input. The parent’s next request contained its own new input. The fork hook reported `source=fork` and the new session ID. Neither its SessionStart event nor its transcript supplied a parent session ID. The adapter leaves that ID unknown on this ordinary CLI path.

One early debug Caller run returned `BINDING_UNAVAILABLE` at startup. A separate 2.1.311 debug control returned `HISTORY_GAP` for an assistant tool-call UUID found in the completed transcript. The cause of the first 2.1.322 refusal remains unconfirmed. A timed 2.1.322 run observed all five requested UUIDs in the file and completed all five bindings. The release-binary Caller run passed. The configured catch-up boundary and its explicit gap behavior still apply.

To repeat, build the release CLI and the adapter integration test with the locked dependencies. Supply the absolute integration-test executable printed by Cargo and fresh SSD scratch paths. The Caller script removes its scratch directory. The Runner script writes its synthetic request and stream files there.

```sh
cargo build --locked --release --bin akasha-otsuge
cargo test --locked --test adapter_context --no-run
python3 scripts/check-claude-caller.py "$native" "$release_cli" "$caller_scratch"
python3 scripts/check-claude-protocol.py "$native" "$adapter_test" "$runner_scratch"
```

Authenticated Claude model behavior and billing remain unverified. The native CLI's synthetic SSE usage estimates describe this fixture's generated messages.

## Fork at a known original message

The official Agent SDK can create a child from a named parent and an explicit message UUID. The SDK rewrites the child's message UUIDs and writes the original session and message UUIDs in each copied record's `forkedFrom`. When Akasha captures the child, the captured raw event preserves that pair. Starting from the first fresh child turn, follow the local parent chain to the nearest copied event to find the exact original cutoff. Reads stay on the child's copied history, including when the parent later changes. The ordinary CLI fork on both tested versions provided only a new session ID and `source=fork`. [SDK fork documentation](https://code.claude.com/docs/en/agent-sdk/python) · [Pinned SDK writer](https://github.com/anthropics/claude-agent-sdk-python/blob/cd08fa086187e7f938775621e39da7ec70995fae/src/claude_agent_sdk/_internal/session_mutations.py#L429-L440)

To make this explicit fork, run the following from the parent session's project using its Claude config directory. Set `PARENT_SESSION` and `CUTOFF_MESSAGE` to the chosen session and original message UUIDs. Creating the copy reads and writes local files. Resume the returned session with the installed Claude integration.

```sh
FORK_SESSION=$(python3 - "$PARENT_SESSION" "$CUTOFF_MESSAGE" <<'PYCODE'
from pathlib import Path
import sys
from claude_agent_sdk import fork_session

child = fork_session(sys.argv[1], directory=str(Path.cwd()), up_to_message_id=sys.argv[2])
print(child.session_id)
PYCODE
) || exit 1
claude --plugin-dir "$AKASHA_CLAUDE_PLUGIN" --resume "$FORK_SESSION"
```

In the controlled native experiment, the targeted cutoff was the first assistant reply. Claude Code 2.1.311 resumed nine marked copies and 2.1.322 resumed eight. Both resumed sessions retained the first user input and excluded the parent's later turns. The old release returned a succeeded capture job with `checkpoint.pending=invalid_frame` on the SDK title records. The fixed decoder consumed both full files in fresh stores and completed both partial checkpoints after an in-place daemon upgrade. Existing raw records are immutable. For a prefix captured before this change, the native SDK transcript still carries the original IDs. The repository fixture projects identities, ancestry, attachment types and synthetic conversation content from the 2.1.322 transcript, and includes both native forms of the title record. [Version observations](sdk-fork-observations.json) · [Recorded SDK fork](native-sdk-fork.jsonl)

```sh
cargo test --locked --test daemon recorded_native_fork_keeps_copied_history_and_branch_isolation
```
