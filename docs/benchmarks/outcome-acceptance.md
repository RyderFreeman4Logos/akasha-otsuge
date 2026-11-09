# MVP-021 outcome acceptance

The source commits `9b590b2`, `40e54bb`, `b25c4e0` and `e596434` implement the
[outcome protocol and command wrapper](../outcomes.md). Final canonical,
independent review and hosted CI evidence remain separate publication steps.

The immutable [archive manifest](results/mvp021/manifest.json) covers 86 files,
103,858 bytes. Its SHA-256 is
`666ba3855c350232fbea07cc3978067aa33193cbd12e3472c30b61e41d7d9e54`.
Every retained log is bound to its launch, terminal and collection receipts.
[Classification](results/mvp021/classification.json) distinguishes behavioral
failures from compile diagnostics and incorrect fixture assumptions.

The first API RED rejected the missing operation. The wrapper RED returned
`INVALID_ARGUMENT` instead of the real compiler's exit 1. The artifact-gap RED
discarded a failed command when an expected output was absent. Each corresponding
behavior is exercised in the final focused suite:

```text
cargo test --test daemon outcome_tests:: -- --nocapture
3 passed; 0 failed; 0 ignored
```

The suite receipt is
[mvp021-boundaries-green-r5.terminal.json](results/mvp021/mvp021-boundaries-green-r5.terminal.json);
its log SHA-256 is
`cf43fa9de326b1a5a4fded1161c4715b095d2afcd522fd2d36d5fcadbc3823bb`.
It verifies immutable/idempotent records across restart; receipt membership;
separate attached, returned and reported causal relations; malformed inputs and
source-hash checks; current permission checks on submit, read, capture and retry.
Reading an authorized source does not attach its unselected memory to a receipt.

The [second build case](results/mvp021/build-case-r2/first.json) invokes the real
Rust compiler recorded in [compiler.txt](results/mvp021/build-case-r2/compiler.txt).
An uncommitted call to an undefined function fails with exit 1. Independent Git
and SHA-256 oracles match the recorded base and dirty diff. Changing only an
untracked input changes the tree digest. Two builds in the same run have separate
artifact hashes and immutable records. A missing executable records a spawn error
without claiming a memory caused it. Expected missing/unsafe artifacts are
explicit gaps, not fabricated hashes or a reason to lose the failure.

The wrapper's 4 KiB previews retain complete source references. The archive holds
all 240,000 stdout bytes and the actual compiler stderr, compressed losslessly.
A separate binary-output regression reconstructs 537,600 bytes across two source
parts through the public paged log API, and checks chunk/range bounds and revoked
access. Duplicate event IDs refuse execution before changing the artifact.

Caller, Consult and delivery acknowledgement are controlled fixtures. The
acknowledgement uses the stored verified binding and the real Domain delivery
operation; a generic CLI identity is correctly denied. This is not a new
authenticated host/model test. A receipt, a reported validation, a hash or a
nonzero exit does not independently prove causation. Outcomes do not change
memories or create incidents; investigation and policy belong to MVP-022.

The first archived build case passed while another fixture assertion failed.
The final second case belongs to the complete three-test pass. All earlier
diagnostics are preserved; passing later tests does not relabel them.

## Independent review and coverage repair

The exact `ee479760` canonical gate passed 461 Rust checks including installed
repetition. Independent [R1 review](results/mvp021-coverage-fix/mvp021-whole-review-r1.md)
also reran the original three integrations successfully, but returned FAIL for
one MODERATE regression: the outcome projection omitted Capture's known reduced
coverage. A zero-byte metadata-only source looked like a complete empty log.

The causal RED reproduced that omission through the public reconcile and outcome
APIs. Commit `e596434` preserves Capture coverage and effective
complete/partial/unknown state in records and pages. A supplied completeness claim
cannot override reduced source coverage. Legacy projections recover coverage
from their fixed Capture revision without inventing a completeness assertion.
The wrapper explicitly reports its observed fully retained parts.

The final focused suite now has **4 passed, 0 failed**, covering all four Capture
coverage levels, explicit partial/complete reports, unknown defaults, downgrade,
restart, retry and legacy records. Both the ordinary wrapper response and paged
logs assert the new status. The [third real compiler case](results/mvp021-coverage-fix/build-case-r3/first.json)
retains those fields and its complete log bytes.

The separate [repair archive](results/mvp021-coverage-fix/manifest.json) contains
34 files, 112,198 bytes, SHA-256
`ce658b23c79c3485d0f54d2128f34be9e18f6ee6e5ed3c13396d202f8dff140d`.
It preserves the earlier gate, failed review, causal RED and both passing repair
runs. The final focused log SHA-256 is
`705e907b8cc7b63e24ee0afd4c6f8ca0fbb5138cc4bc73356a40e048de87d08d`.
These repair results require a fresh exact-candidate canonical gate and whole-range
review; the earlier gate is not silently reused for changed code.
