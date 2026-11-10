# MVP-025 neutral export final evidence

PR #57 merged as 3b6bf74d57b275e154195571700d64406832c2d3 with exactly the
reviewed 7ad2aab5721a2f840865da8a77d9bb367b6d9909 tree
a62bcafa76ebd834cb8d1b06cf528787e64617bc. Issue #25 remains open.

Unmodified just check passed in 511.619s: 545 Rust passes including installed
repeat, 157 daemon cases each run, 34 Python tests, Node/format/Clippy and the
installed lifecycle/private-UID/peer-UID/zero-TCP checks. Full-range independent
R1 passed with zero introduced findings and 26 fresh archive/CLI/storage cases.
Its full unchanged report was published and read back byte-for-byte. CI42758677032
passed at the same head with Cargo cache restored/hit. Gzip retains the original
complete gate log and report bytes; all source hashes bind historical 7ad2aab.

Opaque native captures were removed after exact consumer checks; complete report,
focused log, terminal/runtime and publication receipts remain. The same SSD
worktree and shared target are reused for the import leaf, without a new clone.
Post-merge main CI42760179799 was still pending when this archive was created.

This completes offline neutral export only. Validating import, readonly legacy
migration, actual mempal source acceptance, idempotency/incremental application,
interruption recovery and state/history/receipt round-trip checks remain open.
Earlier causal REDs and focused passes stay in the separate neutral-export
archive. The real write-failure control is EFBIG, not ENOSPC or power-loss proof.
