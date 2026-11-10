# MVP-025 offline import R1 repair

Independent whole-range R1 found F1 on a61acd9: after authority rename succeeds
but parent-directory sync fails, identical replay validated content and returned
success without attempting the outstanding parent sync. The exact public release
probe and full unchanged FAIL report remain archived and published on PR58.
This is a false durability-success result, not observed power-loss data loss.

The new deterministic public CLI test uses a real exported/imported fixture,
then moves that completed owned store back to its recognized unpublished stage.
Mode0300 permits traversal/rename while denying opening the parent for fsync.
First import correctly returns STORAGE_IO after publication; unchanged replay
incorrectly succeeded on the original source. This causal RED compiled and
failed at attempt1 in4.211s. No timing race, injected library or source database
was used; a scope guard restores fixture permissions before cleanup.

The repair makes parent sync unconditional after successful validation/native
close and optional publication. Both new and repeated imports now propagate
that failure; after permissions are restored, replay succeeds with the original
sequence and receipt. Final GREEN passed9archive+5CLI+17storage+1four-phase
SIGKILL test and all-target Clippy in108.782s. Source hashes below bind the exact
repair. Fresh canonical, whole-range R2 and successor CI remain required.

The full R1 report is byte-preserved gzip. Its final output-last-message bytes
hash to e1ee890b289fec8c2e1589f58d2877602cdf4f8f8a0ef94418349b6fff964b1c,
matching the runtime receipt and published body. Its embedded self-reference to
an earlier588a8414 digest does not identify these final bytes; the parent records
this discrepancy without editing the report or discarding its reproduced finding.
All exact native captures were removed after terminal/FD checks; the report,
focused log, provenance and cleanup receipts remain.

The initial canonical PASS belongs to a61acd9 (448.063s,550Rust passes including
installed repeat,34Python tests,157daemon cases per run). It does not waive F1 or
verify this repair. Any initial CI receipt included here likewise belongs only
to a61acd9. The original46-member focused archive's10source hashes must be read
at a61acd9, while the preceding export-final archive remains bound to7ad2aab.

Issue25 remains open for fresh local runtime authority, normal imported daemon
use, real supported mempal migration, incremental imports and domain-level
acceptance. No source database, service, schema, permissions or sidecars changed.
