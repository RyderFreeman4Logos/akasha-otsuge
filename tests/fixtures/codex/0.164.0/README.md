# Codex 0.164.0 native history fixtures

Captured from the installed native Codex app-server with a controlled local
Responses SSE endpoint on 2026-10-28. Native binary SHA-256:
`b93f6811d3f653f0fb83be4050d857fb9abbb111dfa2ce160563327daff7f306`.
These are protocol fixtures, not authenticated model acceptance.

`root.jsonl` contains a real dynamic-tool roundtrip and explicit local compaction;
`fork.jsonl` contains its native fork and a further turn. The native ordinal,
thread IDs, message IDs and fork cutoff are retained. The same running thread
also passed `thread/resume`; this does not prove restart recovery.

Only synthetic prompts/results were used. Native built-in instruction text,
environment blocks and world-state details are replaced with explicit fixture
placeholders; owned absolute paths become `/fixture`. No hidden reasoning is
requested. `manifest.json` binds both original private receipt hashes and these
sanitized files. Native scratch and processes were removed after collection.

Contract references: [app-server](https://learn.chatgpt.com/docs/app-server),
[hooks](https://learn.chatgpt.com/docs/hooks). Rollout fields are version-pinned;
unknown versions or unsupported records must remain observable.

`restart-root.jsonl` and `restart-fork.jsonl` repeat this lifecycle and then
close the first native process, launch a second app-server, resume the root
thread and finish another turn. Both native exits were zero. The resumed root
has 41 rows; the fork still inherits only ordinals below 31. This proves native
process-restart continuity against the controlled endpoint, not authentication.

`exec-root.jsonl` and `exec-pre-tool-use.json` come from native `codex exec`
with one vetted fixture hook and a synthetic shell roundtrip. At PreToolUse,
11 rows were already durable, including the matching call at ordinal9.
The native call ID and its internal turn ID match the actual hook payload;
no later tool result or final answer belongs to that invocation fence.
The isolated invocation used the documented one-off hook-trust flag; global
user hook trust was not changed. This is protocol evidence, not model inference.
