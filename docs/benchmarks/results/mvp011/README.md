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
