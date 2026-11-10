# MVP-023 stdio focused evidence

This leaf extends the local notifier merged in PR #54 (`6eb77a1`). Its final
focused run passes 18 real daemon tests, three standalone file-plugin tests,
formatting, all-target Clippy and diff checks in 73.558 seconds. The daemon test
runtime is 43.90 seconds. No model provider is required for notifier acceptance.

`manifest.json` binds archived receipt/log bytes and final source-file hashes.
Logs are gzip compressed; terminal receipts retain the original uncompressed
SHA-256. Admission receipts for commands beginning with `cargo fmt` describe the
pre-format working diff; final source hashes and the subsequent clean-commit
canonical gate are the stronger candidate binding. This archive does not itself
claim canonical-gate, independent-review, hosted-CI or publication success.

Coverage includes minimal envelopes, external create/update and unsupported
update fallback, stable provider IDs, finite five-attempt retry and restart
during backoff, actual daemon SIGKILL before/after the plugin effect, provider
deduplication and explicit duplicate-risk reporting, live disable, credentials
and read revocation, pinned monotonic attempt deadlines, quiet/digest/priority
policy, malformed/oversize protocol responses, byte-bounded inbox pages,
capacity refusal, detached-descendant cleanup and idle storage stability.
The file reference executable receives 100 grouped incident reports and urgent
escalation, demonstrates one durable failure/retry, updates one message and
retains three generation receipts. Ack remains independent of source review.

Earlier failures are preserved:

- Phase 1 REDs lack external delivery/cancellation. GREEN r1 exposed a retained
  stdin handle; dropping the owned writer fixed EOF. Its concurrent paging
  timeout did not recur, but historical causality remains unknown.
- Retry/crash RED: the first failure terminates instead of retrying; interrupted
  `sending` never recovers. The repaired complete ten-case suite passes.
- Policy RED rejects newly requested fields. The initial GREEN passes 23 config,
  two unit and 12 daemon tests; its one-second timeout fixture could stop during
  capabilities before the assumed delivery PID existed. The corrected fixture
  observes physical delivery under an explicit five-second test budget before
  verifying timeout/reap/retry. Product default remains 30 seconds.
- Reference RED cannot invoke the missing example executable. Its independent
  tests and real daemon grouping/escalation case subsequently pass.
- Byte-page r1/r2 are fixture compile errors (private helper, unsupported hex
  formatting), not behavioral REDs. R3 reaches `RESULT_TOO_LARGE` on valid
  persisted records. The complete final run verifies byte pagination.
- Record-limit RED lacks the explicit capacity reservation. The final run proves
  visible refusal while retaining delivery history and daemon operation.

The reference plugin is a bounded 2 MiB file inbox, not a commercial provider.
Declared provider idempotency is not a universal exactly-once guarantee. These
results do not establish Claude authentication, product stress acceptance,
migration, release packaging or the separate frozen model-quality evaluation.
