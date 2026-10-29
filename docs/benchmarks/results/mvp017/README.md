# MVP-017 evidence

`manifest.json` binds the compact receipts, logs, harness and summary in this
directory. They contain synthetic source text, not private conversations or
credentials. Each command admission identifies its working HEAD; a dirty working
tree is not represented as an exact-HEAD gate. Final whole-candidate gate, review
and hosted CI receipts are published on the implementation PR.

The implementation has three earlier verified commits: `927dcad` (durable text
extraction), `f7afd61` (native executors), and `da97084` (saved-response recovery,
source boundaries and publication deadlines). The final index/acceptance commit
adds explicit rebuilding, documentation and these receipts.

## Actual native extraction

`mvp017-native-r2.json` and `mvp017-native-r2-result.json` record an authenticated
Pi 1.4.0 run using its existing key-command reference, mp-cpi/grok-4.7 through
localhost:48317, and **zero Akasha model profiles**. The exact executable hash is
retained. Source is `da97084` plus the test-only `native-test.patch`, whose digest
exactly matches the dirty-diff digest recorded at dispatch. Five synthetic source
statements yielded six source-linked candidate records in 70.5 seconds including
the harness build/run interval. Every public original read and duplicate-job
replay passed. The roughly 3 MiB disposable profile/fixture was removed; no owned
process survived, and the real Pi settings hashes stayed unchanged.

Parent inspection compared each actual claim/condition/assumption against its
original: the offline condition, private-query prohibition, Alice attribution,
speaker's contrary preference, one-session logging limit and absence of approval
for automated auto-publish all remain explicit. This small sample establishes the
single-CLI path and these cases, not general model quality or Claude authentication.

The first native harness failed before invoking the model because the installed
profile used the `mp` hostname, while the harness required literal localhost.
The owned empty directory was removed. The successful run explicitly targets the
user-authorized localhost route without modifying the original profile.

## Focused validation

- `durability-checks`: 18 job tests, Clippy and formatting pass. Its integration
  filter matched zero tests; `durability-integration` corrected the filter and
  ran all three actual integration cases. Do not count zero-test exits as coverage.
- `recovery-red-r2` reproduces model replay after SIGKILL. `recovery-green-r2`
  passes both durable-response and partial-candidate kill points without a second
  provider call or candidate revision. The first RED used an unsuitable lib-test
  native guardian; the first GREEN had incorrect length-encoded key-range bounds.
  Both are retained as harness failures, not product verdicts.
- `boundaries-red`, `interrupted-red`, `source-limit-red` and
  `publication-deadline-red` reproduce accepted internal sources, overwritten
  revised candidate identity, ambiguous request replay, source-limit propagation
  and writing after expiry. The final durability suites cover their fixes.
- `index-lifecycle-red-r2` reaches the missing public `index rebuild` command;
  `index-lifecycle-green` passes the end-to-end extracted-record lifecycle.
  The first RED reused a source transaction ID for a review transaction; its
  correction is in the separate disposition receipt.
- `index-checks` passes six index library tests, then stops at a busy-reader
  fixture that allowed only one running job. `index-checks-r2` explicitly permits
  two jobs and passes four extraction integrations, 21 retrieval integrations,
  Clippy and formatting. It verifies candidate exclusion, approval, cancellation,
  persisted pause/restart/resume, active-reader rebuild refusal, lost-file rebuild,
  fresh lexical/vector generations, old native-table removal, idempotent rebuild,
  and withdrawal filtering before and after index catch-up.

Earlier `text-checks` retained 18 default-parallel IPC timeouts; bounded isolation
and an observed unchanged default-parallel run passed. The historical cause is
unknown; no product timeout was relaxed. `native-checks` found two existing Codex
privacy/restart tests assuming nine cold capture events fit a two-second binding
window. Those fixtures now await the already supported durable async prefix;
dedicated binding/history-gap semantics remain unchanged. `native-checks-r2`
passes daemon, Hermes and Pi coverage; the preceding run's adapter, Claude,
Codex, CLI and config suites also passed before reaching those fixture failures.
`native-extraction-red` matched zero tests; its `-r2` successor is the causal RED.

The remaining exact command lines, counts, exit codes, original failures and log
hashes are retained verbatim in their bounded receipts. The whole-candidate gate
must still independently pass before publication.
