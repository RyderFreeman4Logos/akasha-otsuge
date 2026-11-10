# Merged neutral-history incremental import acceptance

PR #61 merged as 235c409e8c79766cef84da193d7f6b8338157d58. Its tree equals the
reviewed e4f973de0c963f7cc26d577f735da04739fb682e tree
82e8ef59eab12c8bf79c1798c9f567f1da6008cd. Both local main and the reused SSD
worktree were clean after the fast-forward and next-branch creation.

At that exact source, canonical R4 passed 595 Rust cases with 35 intentional
ignores, including the installed daemon repeat, lifecycle and UID-isolation
checks. It took 455.0589423952624 seconds. Independent conventional full-range
R3 passed with zero actionable findings and its own 71 Rust passes/six child
ignores in 431.72536179982126 seconds. The complete review was published and
read back byte-for-byte:
https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/61#issuecomment-6774692217.

CI42866447962 passed on e4f973d; its Linux job took 11m45s. Full retained logs
prove Cargo-cache restoration and a primary-key hit. A passed restore step alone
was not used as cache-hit evidence. The installed release binary hash in the
local canonical receipt is 5876ef351f1098739ae50d31a5228c8a4a2e936652d9766f6b02b1583fcb3e75.

The historical incremental, incident-fixture, R1 repair and final-drop archives
retain every failure, corrected fixture classification and source attribution.
R1's original full report was overwritten; its reconstruction remains labelled.
R2's full FAIL is preserved. The historical transient admission-lock holder
remains unknown, separately from the deterministic copied-descriptor RED.
No passing result retroactively establishes those unknown triggers.

This closes the neutral-prefix leaf. It does not close issue #25: changed mempal
snapshot identity/update/deletion/reappearance semantics and their acceptance
remain. Product stress/enforcement, packaging, quality/final release and native
Claude authentication are separate. The user's temporary Daybreak/red-team
deferral remains in effect; R3 used conventional review and maintained regressions.

The manifest's historical_source_sha256 entries refer to e4f973d, not later
snapshot-delta source. All necessary reports/logs and publication readbacks are
retained compactly; raw completed reviewer transport was hashed and removed.
