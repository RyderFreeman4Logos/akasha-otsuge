# MVP-019 evidence

This compact archive retains causal failures, focused checks, source identities
and synthetic operator examples. Final exact-HEAD canonical gate, independent
review and hosted CI receipts are published on the implementation PR. Earlier
passing samples are not relabeled as executions of that final commit.

## Real CLI and policy flows

`cli-r1/` contains seven exported examples from nine passing integration tests.
They use the real daemon, private socket, Storage and CLI. A controlled HTTP
extractor returns the same source-backed conditional decision in manual, policy
and automatic modes: manual/policy require operator approval; automatic produces
a delegated active revision. The mode is retained in the record. A job paused
during reload retains generation1/manual, while a new operation uses
generation2/automatic. A separate library test proves pre-policy serialized jobs
retain the original manual contract.

The operator is **synthetic**, driven through a real kernel PTY. Exact-edit
evidence preserves the original proposal, edited draft, reason, before/after
preview and resolved result. Raw IPC and noninteractive apply are refused;
decline makes no change; replay returns the original receipt; a stale new
operation fails. Permission revocation and configuration reload between preview
and confirmation prevent the old preview from committing.

The taste example approves global and project/version rules, verifies an
automatic successor cannot inherit approval or displace the adopted human rule,
then explicitly approves the successor. Actual search returns the local exception
first in its scope and preserves the global rule for another project. Opening a
review leaves active memory usable; explicit rejection retires the latest active
version. A 35-item fixture proves 32+3 pagination and independent state history;
state changes and notification-like `ack` cannot approve memory.

Each exported example records the actual binary hash. Their source is `58a839f`
plus `mvp019-cli-r1.patch` and the admission's untracked review recovery fixture
in `cli-r1-source/`. `mvp019-cli-r1-verified.json` verifies all seven artifacts,
their executable identity and deletion of every owned fixture. The evidence
totals about116 KB; no source corpus or database is retained. These tests are
not authenticated model runs, physical-human attestations or same-UID sandboxing.

## Crash, regression and failure history

Four real SIGKILL checkpoints cover partial staging, pre-commit, commit-pending
and post-commit. Reopen observes source, memory/adoption, review, audit and outbox
atomically; retries commit exactly once or return the existing receipt. These
library fixtures use the trusted internal boundary, separately from CLI checks.

Behavioral REDs retain missing review IPC/CLI, automatic replacement of a human
procedure, missing explicit investigation, retry-policy mismatch after reload,
and extraction publication hardcoded to manual. Their corresponding focused
checks pass. The policy run passes10 extraction library cases,7 then-existing
review CLI cases and4 extraction daemon cases; native authenticated extraction
remains an explicit opt-in test covered by its earlier Issue's own evidence.

The first final focused check passes formatting/Clippy but fails recovery setup
with INVALID_PAGE: an attempted inclusive hash-range fix used a short upper key.
The storage paginator requires equal-length bounds. The corrected range follows
the existing history paginator and includes the largest hexadecimal ID. The
failed receipt is retained with the corrected focused result.

Final focused checks cover all9 review CLI cases, all4 crash points, all-target
Clippy and formatting. The ordinary gate also reruns these paths against the
installed release fixture. Scope limits remain explicit: review state is not
notification delivery; legacy candidates need explicit `review open`; incident
deduplication and broader current-head correction belong to later Issues.
