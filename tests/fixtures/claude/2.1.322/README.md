# Claude Code 2.1.322 on Linux

The official x86_64 executable reported `2.1.322 (Claude Code)`. The runs used a private HOME, workspace, network namespace and loopback SSE fixture. The receipt binds the executable and native output streams by SHA-256. [Native compatibility receipt](native-compatibility.json)

The installed Caller plugin captured a headless turn, two concurrent sessions and a controlling-terminal session. The terminal resumed its prior session, compacted it and captured the next turn. Six durable capture checkpoints identify the original native transcript bytes. A separate test of the native Caller bridge observed five bindings and 13 backfill jobs across startup, resume, fork, subsequent parent activity and an Agent child. Its fixture tool exercised binding. The native Runner made three requests, listed and read context through the real bridge, paired both tool results and delivered its final response.

In the four-turn branch probe, the fork received the parent’s earlier two inputs and its own new input. The parent’s next request contained its own new input. The fork hook reported `source=fork` and the new session ID. Neither its SessionStart event nor its transcript supplied a parent session ID. The adapter leaves that ID unknown.

One early debug Caller run returned `BINDING_UNAVAILABLE` at startup. A separate 2.1.311 debug control returned `HISTORY_GAP` for an assistant tool-call UUID found in the completed transcript. The cause of the first 2.1.322 refusal remains unconfirmed. A timed 2.1.322 run observed all five requested UUIDs in the file and completed all five bindings. The release-binary Caller run passed. The configured catch-up boundary and its explicit gap behavior still apply.

To repeat, build the release CLI and the adapter integration test with the locked dependencies. Supply the absolute integration-test executable printed by Cargo and fresh SSD scratch paths. The Caller script removes its scratch directory. The Runner script writes its synthetic request and stream files there.

```sh
cargo build --locked --release --bin akasha-otsuge
cargo test --locked --test adapter_context --no-run
python3 scripts/check-claude-caller.py "$native" "$release_cli" "$caller_scratch"
python3 scripts/check-claude-protocol.py "$native" "$adapter_test" "$runner_scratch"
```

Authenticated Claude model behavior and billing remain unverified. The native CLI's synthetic SSE usage estimates describe this fixture's generated messages.
