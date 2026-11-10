# MVP-025 imported runtime focused evidence

Product source through 9979fe35a8d0fc480bc97aaded82496647e74e62 initializes fresh
local authority above the original archive boundary while retaining history.
Test-only successor c93428e4f9accfaf3541ab27ca8e764297885934 adds Pi registration
and restart coverage without changing production source.
The exact candidate canonical gate, independent review and hosted CI are pending.
Issue #25 remains open for real mempal conversion and incremental/full acceptance.

R3 passed 59 tests plus all-target Clippy in 126.635s: six runtime tests, one
metadata rejection test, one import crash supervisor, one real daemon integration,
eight archive, five CLI, twenty domain and seventeen storage tests. The two
supervisors each exercised four real SIGKILL checkpoints. One child-only test
is intentionally ignored except when its supervisor invokes it.

The real CLI export/import fixture compares memory, state, history and actual
RecallReceipt fields/output; it starts the imported daemon twice, completes new
capture jobs, observes zero external notifier calls from old history, then sends
one fresh notification after a local review update. Same-archive replay after
those local changes preserves them. Old invocations remain readable evidence;
active lookup/rebinding and all three persisted native registration token paths
are refused. Fresh local tokens, bindings, jobs and notification dispatch pass.
No actual host/model authentication or large production load was exercised.

R4 passed both real daemon protocol fixtures plus all-target Clippy in 7.047s.
The additional controlled Pi fixture re-registers the same imported native
session, receives a different token, binds a fresh invocation and preserves
both across restart. The original token is denied before and after restart.
This tests native protocol handling, not actual provider authentication.

## Failure ledger

- runtime red-r1: fixture setup RESOURCE_BUSY while admitting into a paused
  source. The fixture now admits first, then pauses. This was not causal RED.
- runtime red-r2: causal generation reuse and an imported job claimed after a
  local revision. Both initial focused regressions turned green.
- capabilities red-r1: causal old active invocation and acknowledged old
  notification dispatch. Its token fixture had an invalid native layout.
- tokens red-r2: repaired layout proves an imported Codex token admitted a new
  capture job. The final fresh-token controls cover Codex, Pi and Claude paths.
- capabilities green-r1: five focused tests plus Clippy passed.
- daemon red-r1: fixture INVALID_PAGE from a history range beginning at zero.
  Its collected receipt was mislabeled causal; the explicit diagnosis corrects
  that interpretation without rewriting the original receipt.
- daemon red-r2: valid fixture reaches IMPORTED_RUNTIME_DISABLED at startup.
- green-r2: compile failure from the old offline-only assertion after removal
  of the startup block; the assertion now checks the durable import boundary.
- green-r3: all selected checks passed with complete logs retained below.
- host-green-r4: both daemon protocol fixtures and Clippy passed.

Archive file hashes bind all compressed log bytes and original receipt bytes.
Source hashes bind c93428e, whose production source is identical to 9979fe3;
docs updated afterward are not claimed as part of that tested source. The R3
and R4 receipts retain their separate run identities and test scopes.
Historical PR #58 main CI42770214112 is retained
separately and is not CI for this runtime candidate. The small real-sample note
is preparation only, not a migration or source-content verification claim.
SIGKILL is not power-loss or real ENOSPC evidence. Cross-UID scope remapping and
incremental application of different archives remain outside this leaf.
