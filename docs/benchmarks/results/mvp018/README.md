# MVP-018 evidence

This archive binds source identities, commands, observed usage, native discussion
sources, candidates and cleanup. The small samples use synthetic originals; no
private conversations or credentials are included. Final whole-candidate gate,
independent review and hosted CI receipts are published on the implementation PR.

## Actual native samples

The single-Pi sample in `mvp018-native-r5-result.json` passed with four source-linked
hypothesis candidates. It used Pi1.4.0 and mp-cpi/grok-4.7 via localhost:48317,
existing authentication and zero Akasha model profiles. It stopped at the shared
output budget after three completed phases and four starts; this is not a claim
that both cross-questioning rounds completed. The successful case is retained
inside a failed combined harness: the second case reused the same owned profile
for a different daemon and was correctly refused with ADAPTER_INSTALL_CONFLICT.
Separate single/mixed selectors now isolate each adapter installation. No repeated
single-model call is needed to turn that wrapper green.

Parent inspection checked the actual originals, reports and candidates: offline
keyword availability, private-reranker restriction, missing latency and missing
approval remain explicit. Suggested rollout gates remain assumptions, not human
approval. Role differences and the dependent question's unresolved prerequisite
are preserved. Scope and source revision/hash/UTF-8 checks pass. These are two
questions over three originals, not a general model-quality claim.

`mvp018-native-r6-result.json` passes the isolated mixed sample: Pi handles
interviewer/expert, and native Codex0.164.0 supplies the independent reviewer
answer. Pi's next round critiques that actual Codex answer against the originals.
Four candidates retain all disagreements and unresolved dependencies. The shared
output budget stops before reviewer round1 completes; the five starts and31,465
reported tokens are observed usage, not a currency estimate. Codex uses its
existing native profile; the artifact does not attest a particular Codex model.

R5 is source `8becbee` plus `mvp018-native-r5.patch`; R6 is the same base plus
`mvp018-native-r6.patch`. Each patch digest matches its dispatch. The latter adds
the duplicate-dependency guard and isolated harness. The single sample's valid
DAG does not use the rejected duplicate edges. Native executable hashes are
retained separately. Neither sample is relabeled as the eventual final HEAD.
Each disposable profile/fixture occupied about3 MiB and was removed; original Pi
settings and Codex configuration hashes are unchanged. No owned consumer remains.
The three final library tests, including the causal dependency RED/GREEN, pass.

## Validation and failure history

Config/legacy checkpoints, controlled native role isolation, cross-questioning,
round/start/token stops, forbidden tools, actual process cancellation, policy
publication, and real SIGKILL at saved-phase/partial-candidate boundaries pass.
Recovery retains the original clock and usage, publishes no duplicate revision,
and never replays an ambiguous in-flight native call. Manual, policy and automatic
publication do not become human-confirmed. A causal duplicate-dependency regression
protects bounded resolution of the model's question graph.

Native R1 was rejected by the strict report parser; its rejected text was not
exported and its exact cause remains unknown. R2/R3 failed in diagnostic-test
compilation and made no model request. R4 exported the rejected expert response:
answers.question contained question text instead of the required ID. Prompt and
schema now state the exact-ID contract; the parser still rejects text and unknown
IDs. R5's successful single case exercised that clarification. All native scratch
roots were removed only after their owned consumers exited, and original profile
configuration hashes stayed unchanged.

Other focused failures are retained: missing config/IPC REDs, a private test API
compile error, an unused import, the causal unresolved-answer authority RED, a
RevisionRef test error, and Clippy's test-module placement error. None is silently
relabeled as an all-passing command. See the verbatim logs and dispositions.

The final focused run passes four controlled daemon discussions (two authenticated
selectors remain opt-in), all-target Clippy and formatting. Its receipts are
separate from the subsequent exact-HEAD canonical gate.
