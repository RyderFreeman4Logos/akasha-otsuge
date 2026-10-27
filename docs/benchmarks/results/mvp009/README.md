# MVP-009 immutable receipts

See [the acceptance report](../../../providers-acceptance.md) for measured scope,
source/release identities and limits. Run `sha256sum --check SHA256SUMS` here to
verify all 148 preserved original files. The manifest excludes this explanatory
README and itself.

- `mvp009-gate2.*`, `mvp009-review-r2.md`, `mvp009-long-r2*` and
  `mvp009-gb10-r2*` are the repaired source's final gate, review and manual results.
- `mvp009-focused-evidence.json` classifies the initial regression sequence;
  `mvp009-r1-fix-evidence.json` classifies the review repair. Their named logs and
  terminal/admission receipts retain both successful and failed attempts.
- `mvp009-review-r1.md` is the superseded FAIL report. `gate1`, `long-r1` and
  `gb10-r1` keep the earlier executable's results without transferring acceptance.

Focused dirty-tree receipts name their then-current base HEAD; they do not prove
that the committed base contained the repair. Exact frozen gate receipts supply
that attribution. Local absolute paths and historical process IDs describe the
original execution environment; they are not instructions to reuse those paths
or processes. No expanded corpus or executable is archived.
