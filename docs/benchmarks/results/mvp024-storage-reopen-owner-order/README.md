# Controlled storage reopen: owner error ordering

The final R1 repair control exposed another first-error path on 622fc8a: an
owned task had already failed and been collected during close, but a later
accept error replaced it. The regression failed with IO_ERROR rather than
JOB_ERROR. This is a product RED, separate from the prior accepted-first and
scheduler-first controls.

The shared close path now records each collected owner error immediately in
the transition's first-error latch. Storage-close errors use the same latch;
all owners, index and storage still close. Ordinary callers of close retain
their supplied primary outcome through a fresh latch. No new dependency,
configuration or retry is introduced.

The expanded maintained test exercises accept-first, scheduler-first and
owner-first ordering, asserts complete owner drain, and proves storage closed.
The focused run passes 11 library lifecycle tests, 17 daemon budget cases and
both retrieval-generation controls (30 executions), plus all-target Clippy,
plan validation and whitespace checks. Controlled task panics are test inputs.

Historical 622fc8a evidence is retained separately: its 440.424-second local
canonical gate passed 613 Rust executions/35 intentional ignores, Python/Node
checks and installed lifecycle/UID controls. Hosted CI42949516550 failed at
the unchanged Codex caller's initial bind with HistoryGap. That exact selector
passed in the local canonical and once in isolation. The failure's runtime
cause remains UNCLASSIFIED; neither result erases it or justifies blind retry.
The failure preflight pins unchanged owning source/fixtures and lists CI phases
that were not reached. A new candidate's CI is independent evidence.

The previous full R1 FAIL report and earlier failures remain in the linked
[review-fix archive](../mvp024-storage-reopen-review-fixes/README.md). Its source
pins now explicitly refer to historical 622fc8a. This archive's manifest pins
the affected source/test bytes at historical 0de2704; admissions precede formatting and
are not interchangeable with those final source pins. The committed
canonical gate, independent review and PR #63 merge subsequently passed; see
the [final leaf archive](../mvp024-storage-reopen-final/README.md).

Measured-RSS admission, cgroup enforcement and full #24 scale acceptance are
unfinished. No Daybreak, new red-team campaign, private corpus mutation or
provider/authentication acceptance was performed.
