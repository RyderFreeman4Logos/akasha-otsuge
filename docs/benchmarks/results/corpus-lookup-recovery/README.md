# Corpus lookup and interrupted-build recovery

`summary.json` separates passing diagnostic cases from the failed full-size
preparation attempt. The source and binary identities are retained. Mutable
authority uniqueness constraints are unchanged; only the immutable imported
ordinal lookup uses ordinary equality, with duplicate/read/coverage checks.

`raw-receipts.tar.gz` contains unchanged raw requests, logs, observer samples,
state and test results. `archive.json` gives its hash and every member's SHA-256.
It excludes executable copies and databases. The full initialization snapshot
precedes explicit abandonment; its timeout is not a successful index build or
a hard backend failure. Full replacement/abort performance remains unmeasured.

The earlier ordinary-lookup baseline rejects deliberate duplicate injection
because it has a uniqueness constraint; this is expected, not a correctness bug.
`mvp-003-index-abort-native-r1.log` is a test compilation error from a misspelled
SDK method. The corrected native test and ten release tests pass. The controller
checks exercise an actual older binary's blocked build, invalid operation IDs,
successful cleanup and unchanged completed reuse. Twelve further checks verify
exact stopped ownership, native effects surviving process crashes, abandonment
without artifact changes and successor reconciliation. Process SIGKILL does not
simulate physical power loss.
