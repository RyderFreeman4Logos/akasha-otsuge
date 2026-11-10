# Incident fixture repair after incremental canonical failure

The [classification](mvp025-incremental-gate-classification-r1.md) retains the
failed exact canonical gate, its unreached stages, non-reproducing diagnostics,
a deterministic negative control and final focused/Clippy results. The historical
full-suite trigger is still unclassified; a new exact canonical is required.
The fixture now tests an already admitted but unlinked job after its unchanged
deadline expires, retaining the real SIGKILL and all original recovery assertions.
No production change from the negative control remains.

The separately collected base/main CI42815108137 passed at fe30c78 and saved
the dependency/build cache after a cold miss. It validates the merged mempal
reader, not this incremental candidate. Its full compressed log and hashes
are included as dependency evidence; the earlier final-reader archive's pending
main-CI statement was correct at its capture time.
