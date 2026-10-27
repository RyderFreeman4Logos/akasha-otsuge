# MVP-010 independent-review repairs

These are byte-identical development originals. Runs used dirty worktrees;
admission/terminal HEAD identifies the parent commit, not the full tested tree.
The final committed-candidate gate, fresh review and CI are separate evidence.
`runs.json` classifies each attempt without treating a filename containing
`green` as a passing result. The R2 RED collector's initial classification was
`inspect-terminal`; its log demonstrates the three actual reader escapes.

The immutable R1 report found five regressions. R1 sync authorization now covers
serialized per-record reads, provider admission after queueing, durable progress,
activation and final completion. R2 receipts retain owner-wide diagnostic scope.
R3 explicitly installs both protobuf-compiler and libprotobuf-dev; the original
hosted failure is run 40573097722, job 122432233913 (missing empty.proto), log
SHA-256 d217ba6381cced88f879c0a33235b83037b9c7e4daf18f382151bf63cd081d3b.
R4 retains failed conversion/pooling diagnostics across restart. R5 permits legal
wrapped records in internal rule scans while preserving public/output budgets.

R1's first two attempted GREENs did not compile. Its third run passed 22 library
and 29 daemon cases but failed the fake HTTP server's cleanup after cancellation.
The corrected fixture permits only expected BrokenPipe/ConnectionReset in its
retrieval mode; the no-forbidden-send and zero-watermark assertions are unchanged.
The fourth run and subsequent module extraction both passed 22 library and
30 daemon tests. The extraction preserves the actual tests; moved lifecycle and
rule tests have new module-qualified names, while manual model acceptance names
remain unchanged. Capacity JSON reports conservative offline tokenizer results.

R4's three cases use zero vectors, finite f64 values that overflow f32, and
opposite chunk vectors whose average is zero. R5 creates legal near-128-KiB
Facts through Domain, including an ancestor-scope case, plus a human-reviewed
large rule whose whole-entry omission must report a character-budget gap.

All model traffic is synthetic and local. These receipts do not claim real-model
quality, the 50-GB/full-concurrency product acceptance, or production performance.
