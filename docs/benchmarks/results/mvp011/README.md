# MVP-011 focused development receipts

`focused-receipts.tar.gz` contains original admission, log, terminal and collected
receipts. `manifest.json` hashes every original and the archive. These runs used
dirty development source on base `d5661016c3bfb057f4b01799035ef84c45d8450c`;
the admission HEAD identifies that base, not an exact candidate. The final
committed-HEAD full gate and independent review are separately published in the
implementation PR before Issue closure.

The final focused run `mvp011-focused-green2` passed all-target Clippy, 19 Rust
contract tests, the 60-task reference integrity check and five Python negative
controls. Its log SHA-256 is
`cb21da44f5d41e28f43ac9d895978d51be03c3da8b558aa46595bf47091f9d37`.
The 16 Caller/Runner combinations are cases within the Rust matrix test, not
16 real model/host executions.

Behavioral REDs cover invalid hint normalization/framing, budget depth/deadline,
Runner terminal and permission rules, absent durable binding, missing lineage,
workspace escape, constant capability tokens, failed/EOF handshake reuse,
missing invocation precision and absent reference validation. Their GREENs use
the production shared implementation. Compilation/setup failures are preserved:
the initial budget import, crate-private unused helpers, generic ConfigManager
bounds and unclosed RunSpec brace were not behavioral REDs. The first aggregate
focused run passed Rust/Clippy but reached reference validation before its
manifest was written; its failure is not counted as a passing gate. The next
unchanged Rust run passed with the completed manifest.

No host authentication, real model evaluation, production consult workflow,
RecallReceipt publication or 1700-second real Runner run is claimed here.
See [the contracts](../../../adapters.md) and
[the frozen reference rubric](../../../evaluation/reference-v1/README.md).

`freeze-pin-receipts.tar.gz` and `freeze-pin-manifest.json` retain the subsequent
frozen-base negative control: the original verifier from commit `7356d17` accepted
three coordinated input/manifest rewrites. The repaired verifier pins the original
manifest and refuses source, model-profile and rubric resealing, including a
version-bumped empty replacement. The original reference directory is unchanged.
`mvp011-focused-green3` passes 19 Rust tests (including the new grandchild path),
seven Python reference controls and all-target Clippy; log SHA-256 is
`b410885d8b98db0ca3965f62db5c06f6dabb8229f28f985a87dc80b9fc000a84`.

`r1-repair-receipts.tar.gz` and `r1-repair-manifest.json` preserve the complete
independent R1 FAIL report, original passing pre-repair full gate, and original
repair runs. The controlled-clock RED returned WorkflowTimeout where the earlier
deadline required RunnerTimeout. The oracle RED admitted S04's successful no-hit
after whole-grant revocation; the erratum control admitted coordinated resealing.
The repaired shared guard retains the effective deadline's trigger and cancellation
precedence. The [separate pinned erratum](../../../evaluation/reference-v1-errata-1/README.md)
corrects S04 while preserving the original v1 bytes and scoring population.

The first deadline GREEN attempt was blocked before tests by an sccache0.17.0
client talking to the existing0.19.0 daemon. The next run pinned the already
installed0.19.0 client, verified against that daemon's executable, and reused its
cache without restart. An intermediate focused run failed only a Rustfmt line
wrap; its original log remains. Neither failure is behavioral RED or a green gate.
Final focused run `mvp011-r1-focused-green2` passes20 Rust contract tests,9 reference
tests and all-target Clippy in9.404s; log SHA-256 is
`45acecbcecb3c1bf83acd0bc61e956866a9ef67ed7a100db20340be09a23acb1`.
These repair receipts identify dirty development based on617636a; they do not
replace the subsequent exact-HEAD full gate and whole-candidate R2 review.
