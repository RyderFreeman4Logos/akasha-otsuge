# MVP-023 R1 review repairs

The prior stdio archive's source hashes bind candidate `cde76abe7e8a45288d06919a93a7d06f940c10ee`,
not the later repaired tree. Its exact canonical gate and hosted CI passed, while
independent R1 review found two introduced defects. The immutable full report
was published and read back on PR #55; it remains FAIL.

The new RED reproduces false success from malformed message state, a dangling
receipt and a missing receipt, plus permanent failure after policy cancellation
on the fifth claim. The repair validates all retained reference-plugin entries,
their referential integrity and the current message/receipt hash. Historical
receipts may still reference a message that has since been updated. Invalid
state remains byte-for-byte unchanged.

Notifier claims and transient failures now have separate persistent counters.
Disable, read revocation, route change and shutdown block without consuming or
renewing the failure budget. A real fifth-claim cancellation test verifies both
successful sixth-claim resumption and exhaustion after the next transient failure;
restart preserves the outcome, original start and exact envelope/key. A unit
checks all four control codes and migration of older terminal control records.

The complete focused run passes four Python cases, three policy units, 19 daemon
notification cases and Clippy in 128.895 seconds. The new manifest binds current
source hashes and archived bytes. This evidence does not substitute for the
repaired clean candidate's canonical gate, whole-range R2 review or hosted CI.

Native reviewer transcripts were not inspected and were removed after exact
consumer exit. The final report, test log, hashes and public readback remain.
