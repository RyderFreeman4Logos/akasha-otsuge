# Durable history and actual recall receipts

MVP-006 supplies library APIs on the real pinned SurrealKV authority store.
Its [production CLI/IPC](daemon.md) is provided by MVP-008;
retrieval indexes and real host binding are subsequent dependent Issues.

## Historical reads

`Domain::get(context, memory_id, Some(at))` returns `Absent`, `Active`, or
`Retired` at a committed sequence. `None` selects the current committed point.
The returned `ReadResult` always identifies `read_seq`, `validated_at_seq` and
the actual `retrieval_mode`. Historical results remain historical evidence after
a correction or retirement; current reads cannot return an old adopted revision.
A pending or rejected proposal does not erase the prior adopted decision.

`state(context, scope, at, cursor, PageBudget)` enumerates active memories at one
fixed point using **exact scope equality**. A global scope and a project/version
exception are separate queries. This administrative enumeration is not a ranked
applicability search. The later retrieval workflow combines applicable scopes.
Adoption, retirement and fixed-length scope membership are committed in the
same authority batch as the Memory revision, receipt and outbox. State IDs are
`scope/{sha256(canonical Scope JSON)}/{sha256(memory_id)}`. Every returned member
is checked against its immutable Memory and adopted pointer at the same point.

`history(context, memory_id, from..=to, cursor, PageBudget)` reads that record's
ordered immutable revision keys. The inclusive range starts at 1 or later and
ends at a committed point. It includes candidates, rejection, retirement,
source spans, approvals and previous/adoption links. `status` is evaluated at
`to`: an older active revision is `superseded` when a later adoption or retirement
exists. An old raw revision's recorded lifecycle is never rewritten.

Both pagers process at most 64 entries per call, plus one native key of lookahead,
and enforce a caller-selected serialized-item byte budget of at most 1 MiB.
The budget covers item JSON, with separately bounded envelope/cursor overhead.
State's limit bounds examined memberships, not just emitted active rows. Future
or retired memberships can produce an empty page **with a next cursor**; only a
missing next cursor indicates the end of that range. Oversized first items return
`RESULT_TOO_LARGE`, and callers needing a complete result must consume all pages.
Cursors bind operation, memory/scope and commit range; they survive restart and
later writes. They are query positions, not authentication capabilities.

No history call replays all source events, copies the database, holds an ephemeral
snapshot across requests, or filters current ANN results to imitate old vectors.
This issue implements state/history modes, not historical ANN. A RecallDraft
requesting `historical_semantic` returns `HISTORICAL_SEMANTIC_UNSUPPORTED`.

## Record time and live permissions

Each commit now stores one immutable `clock/{seq}` observation in the same native
transaction. `TimedMemory.recorded_at_ms` is this system-clock millisecond value,
distinct from original event time, observed source time and optional reality
validity. Clock adjustments can make wall time non-monotonic; `commit_seq` remains
the canonical order. No timestamp-to-sequence shortcut or retroactive clock
backfill is provided. A at sequence 10 and B at 20 remain distinct even if B
declares an earlier `valid_from_ms`.

`ReadContext::from_verified_binding(principal, binding, run_id)` is a trusted
server constructor, **not an authenticator**. It cannot be deserialized from a
worker payload. The binding is a nonsecret audit identifier, never a bearer token.
`set_read_grant` is an administrative server API storing a principal's current
scope ceiling, or revocation (`None`), with optimistic revisions. A principal
without a live grant is denied. Host/control adapters must authenticate these
operations and map invocation capabilities to server-established principals;
raw Storage and the original internal Domain reads are not public worker APIs.

Authorized reads and receipt preparation share Storage's writer lease with grant
changes. The live grant and current adoption checks therefore cannot race a
concurrent authority commit inside that boundary. Multiple Domain instances over
the same Storage share the lease. `validated_at_seq` records the current permission
check point; it does not change the requested historical `read_seq`. A scope
shrink can deny old data without altering any past semantic state. This is a
library boundary, not OS isolation against an arbitrary same-UID shell.

## RecallReceipt and delivery

The server supplies a bounded `RecallDraft` containing its **actual serialized
UTF-8 response**, read point, selected revisions, extra source spans, output
status, declared scope/coverage, truncation and config/index context. The draft
is not a model-deserialized authority declaration. Preparation verifies selected
revisions and all source ranges, adds selected memories' evidence and approval
sources, and rechecks live permission. `current_state` selections must still be
active at the final check; historical selections resolve at the requested point.
`revision_history` may cite a non-active immutable revision explicitly.

`prepare_recall` writes the exact output once as immutable Evidence, and a compact
`RecallRecord` as State, in one durable commit. Receipt ID equals request ID.
The record includes original principal/binding/run, a canonical request hash,
selected revision references, all source spans, body byte length and SHA-256,
read/validation/permission points, scope, coverage/truncation, status, configuration
generation, optional index generation, pinned engine identity and storage format.
An index generation is the server's identity of the index actually used; the
product index builder and verification of its real coverage belong to MVP-010.
When no index was used, the server supplies `None`.

Output is at most 128 KiB; selections at most 32 and final distinct source spans
at most 64. Metadata is also subject to Storage's 128 KiB record limit. Invalid or
oversized requests fail before commit without truncating silently. `no_hit`
requires complete declared coverage and no selected evidence; missing coverage
or response truncation requires `incomplete` and explicit gaps or a truncation
reason. `no_hit` is not a claim that a fact does not exist outside that coverage.

The initial delivery phase is `prepared`. The server may call `record_delivery`
only after its transport confirms the complete exact response; it must supply
the matching body hash and a stable delivery request ID. This appends a phase
revision without copying the body or changing original retrieval metadata.
An acknowledgement can record a delivery that already happened even after a
subsequent permission withdrawal; it exposes no body and does not authorize a
new send. A lost/partial transport acknowledgement remains prepared/unknown.
MVP-008 acknowledges archived output only after the CLI writes and flushes it.
The library alone does not establish that an agent received the bytes.

Identical preparation and delivery retries return their existing CommitReceipts.
Changed content/binding is refused; preparation also rechecks live permissions
and selected current heads before reuse. A retry preserves the original receipt's
validation point rather than rewriting history. A new invocation uses a new
request ID and its own receipt.

`recall_receipt` returns the immutable bytes and original metadata after checking
the original binding and current grant. Its outer mode is always `receipt_replay`;
the stored retrieval mode and original validation point are preserved inside the
record. Later index/config context, memory correction or retirement cannot change
those bytes. Replay is not current advice or an attempt to regenerate a model's
answer. `revision` and `recorded_at_ms` identify the most recent receipt phase;
`prepared_seq` identifies the original preparation commit.

## Format and verification boundary

Storage format 2 makes commit clocks and scope membership mandatory. Format 1
development directories/backups are refused unchanged; there is no silent
in-place migration. The separate full-corpus spike directory is untouched.
See [storage.md](storage.md) for the fresh-directory migration and restore policy.

`cargo test --test domain --test storage --locked` passed 17 Domain tests and
17 Storage tests during development. Seven new Domain fixtures use the formal
native engine: exact 10/20 points, backdated validity, superseded/candidate/rejected/
retired views, fixed-point pages after writes/reopen, current grant revocation and
queued scope shrink, byte/scan bounds, empty continuation pages, approval sources,
actual output byte/hash replay, distinct index-context fixtures, phase metadata
integrity, idempotence, native checkpoint/restore and explicit incomplete status.
An added Storage fixture verifies durable as-of and clock reads. The retained
cancelled-commit/checkpoint regressions also passed after sharing the writer lease.

The metadata integrity fixture first reproduced acceptance of a phase update
that changed the original index context; reads now compare it with the immutable
prepared record and reject it. Fixture index identities are not a real LanceDB
switch. No real host/transport delivery, model replay, installed service, 50 GB
product workload, historical semantic retrieval or format-1 migration is claimed.
Candidate `e98d4cb` passed the exact-HEAD canonical gate and independent whole-
candidate R1 review (zero findings). Linux CI40454932598 passed; PR #34 merged
as `42035bd` and #6 closed completed. Native as-of lookup traverses a key's version
history; bounded application pages do not imply constant-time hot-key lookup.
