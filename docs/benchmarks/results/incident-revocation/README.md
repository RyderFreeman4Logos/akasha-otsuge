# Incident read-root revocation fixture

The normal lifecycle test restored read permission when the native runner PID
disappeared. It had already released that runner to return a successful editor
result, so PID exit did not prove cancellation or durable workflow completion.
A canonical run on maintenance candidate `5163147` observed the memory change
and incident resolution after this early permission restoration.

The owning test and authority/lifecycle code are byte-identical to main
`9f2ba15`. The separate repair starts from that main revision and keeps the
existing editor barrier closed for `roots_off`: the daemon must cancel the
blocked runner while read roots remain denied. All original `PATH_DENIED`,
unchanged-memory, single-start and no-replay assertions remain. Production code,
permissions and deadlines are unchanged.

[Evidence manifest](evidence-manifest.json) verifies every member of
[evidence.zip](evidence.zip), including the original failed gate, source custody,
the unchanged selector passing once, the exact repair patch, and focused checks:
nine incident tests plus 17 editor tests passed, with two opt-in native cases
ignored; formatting and all-target Clippy passed. These controlled protocol
fixtures do not establish authenticated model-host acceptance. The failed run
did not capture worker phase timestamps, so its exact scheduling is unavailable.

The earlier installed serial-suite 300-second timeout is a separate unresolved
observation. A subsequent unchanged isolated installed check passed in 251.03
seconds; neither result explains the other. This repair does not claim to fix
that timeout or production memory/scale behavior.

Final exact-tree canonical checks, independent review, hosted CI and merge are
still required. Their immutable receipts and full report belong to the PR
publication record. Full MVP-024 acceptance remains tracked by
[Issue #65](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/65).
