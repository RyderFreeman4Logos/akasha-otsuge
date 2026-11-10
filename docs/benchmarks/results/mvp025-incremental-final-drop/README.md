# Native shutdown and admission lifetime repair

At historical HEAD 4a542e3, canonical R3 passed 592 Rust cases (34 ignores),
installed lifecycle/UID checks and CI42836962444. Independent full-range R2
nevertheless found one HIGH regression: native Tree::drop schedules asynchronous
cleanup after its wrapper releases the directory descriptor. Descriptor reuse
can redirect that cleanup. The complete R2 FAIL and its test log are preserved.
The two published report parts concatenate to the exact original report:
https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/61#issuecomment-6771073285
and https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/61#issuecomment-6771073902.

Native storage now owns a current-thread runtime on one thread per open store.
It constructs the native tree there and retains the directory until native close
and runtime teardown finish. Explicit close requests use one bounded channel;
dropping a caller's waiter does not cancel an admitted close. Dropping the last
shared native handle closes the channel and completes cleanup on the owned runtime.
The maintained final-drop regression failed before this repair. A separate
ordinary test transfers a store across caller-runtime shutdown, then commits,
reads and checkpoints it. The native dependency and database format are unchanged.

Focused R1 passed all 69 Rust cases, then failed one Clippy needless-question-mark
check. Focused R2 failed two existing recovery tests with IMPORT_BUSY. Initial
instrumented diagnostics passed, including ten complete storage-suite runs; a
narrower diagnostic reproduced transient directory-lock contention on attempt 8.
Its immediate /proc/locks lookup had no remaining holder. That snapshot does
not identify the historical holder.

A deterministic descriptor-copy control proves a concrete admission defect:
dropping Admission leaves its flock active until every duplicate open description
closes. Linux fork inherits the same description before exec. Both admission
locks now explicitly unlock when their owner exits, including partial acquisition
failures. The control fails before the repair and passes afterward. This fixes
that proven class; it does not retroactively identify the earlier transient holder.
All temporary diagnostic changes were removed exactly.

Final focused R3 passes 71 Rust cases with six intentional supervisor-child
ignores, formatting, all-target Clippy and diff checks. This is dirty-candidate
evidence; a successor exact-HEAD canonical, independent review and hosted CI
remain separate admission requirements. The manifest pins the repaired sources.
Older archives retain their original commit identities.

The user temporarily deferred Daybreak and red-team work. The maintained
regressions cover already confirmed defects and normal lifecycle behavior;
no Daybreak or new red-team campaign was launched for this repair. Product-scale
resource enforcement, changed mempal snapshots and actual Claude authentication
remain unverified. Issue #25 stays open.
