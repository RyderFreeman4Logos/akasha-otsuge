# MVP-025 offline neutral import focused evidence

This independent leaf restores a validated archive into a fresh offline authority
store, preserving original byte content, all revisions, clocks and receipts.
Recognized interrupted stages resume without duplicate commits; completed identical
imports preserve later local work. Unrelated destinations and staging directories
are rejected. Daemon startup refuses imported runtime authority before configuration,
permissions, jobs or notification recovery. It does not complete issue #25.

Initial RED compiled two public tests and failed because archive import was absent.
GREEN R1 failed compilation (unused import and SHA-256 output formatting), not a
behavioral RED. R2 passed all six archive tests. Additional cleanup/daemon controls
then failed for retained rejected staging and automatic daemon activation; R3 passed
seven archive tests after their repair. R4 passed eight archive, five CLI, seventeen
storage tests, the four-phase SIGKILL test and all-target Clippy. A new unowned-stage
control failed before its identity guard; R5 passed the same 31 tests and Clippy.
A permissions negative control then proved a recognized resumed stage could be
published with 0755 mode; it failed before the ownership/mode repair. Final R6
passed 31 tests plus Clippy in 49.658 seconds. The foreign-stage fixture was tightened
to 0700 with an explicit IMPORT_CONFLICT assertion so a permission refusal cannot
mask the separate missing-identity guard. No earlier receipt is relabeled.

SIGKILL phases are durable record staging, sequence-ordered head reconstruction,
before publication and after publication. Each resumes to sequence3 and repeats
without duplicate commits, with original historical views, receipts and clocks.
These controlled native tests do not establish power-loss or kernel ENOSPC recovery.
Input cases include unknown versions, changed blobs, forged references/request
hashes, unknown fields, offsets and oversized lines. Tests preserve source bytes,
existing destinations, private modes and later local writes. A missing or incomplete
initial ownership marker is intentionally refused instead of adopting unknown data.

Source hashes bind the final formatted files below. Admission receipts bind their
own earlier HEAD and working patch, not an exact committed candidate. Final canonical,
whole-range independent review and hosted CI are separate required stages.

The runtime map is byte-preserved gzip: source-only discovery at 415f50c, with five
Git blob identities verified and exact native captures removed after terminal/FD
checks. Its recommendations are not implemented runtime initialization or live
migration evidence. The actual daemon guard is separately covered by two startup
attempts with unchanged application sequence. Historical main CI42760179799 is
successful evidence for merged export3b6bf74, not this import candidate.

Fresh runtime authority, domain-level active-state/receipt replay, incremental
archives and real supported mempal sample migration remain outstanding. No old
mempal database, service or sidecar was changed. Test fixtures were removed by
owners; only shared build inputs and compact evidence remain.
