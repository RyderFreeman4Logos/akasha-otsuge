# Evidence-bound memory editing

The editor investigates a reported problem through a configured native Runner.
It can propose a new memory revision, open a review, or report a non-memory cause.
The daemon checks evidence, scope, current permissions and expected revisions
before publishing a patch. Original source records are immutable.

## Export and submit a patch

```sh
akasha-otsuge memory export MEMORY_ID
akasha-otsuge memory patch patch.json --idempotency-key correction-1
akasha-otsuge patch show correction-1
```

`memory export` returns the latest proposal and its `revision.commit_seq`; this
can be a pending candidate rather than the currently adopted revision. Copy its
`record.draft` into the patch and edit only the intended fields:

```json
{
  "scope": {"user": "uid:1000", "project": null, "branch_or_version": null, "session": null},
  "changes": [{"memory_id": "MEMORY_ID", "expected_revision": 42, "draft": {}}],
  "reason": "The source establishes Linux only; other platforms were not tested.",
  "evidence": [],
  "expires_at_ms": 1794000000000
}
```

The empty draft/evidence and sample revision/expiry are placeholders, not an
executable patch. Supply the complete exported draft, actual current revision,
future Unix-millisecond expiry and verified source spans. Each span identifies
an event, immutable revision, SHA-256 and exact byte range. A patch needs 1–32
evidence spans, a nonempty reason of at most 4,096 bytes, and 1–8 distinct targets;
the complete patch is limited to 64 KiB. Every draft must cite patch evidence.

All changes, adoption/review records, audit and derived-work outbox entries commit
atomically. A stale target returns `REVISION_CONFLICT` without partial changes;
an expired new patch returns `PATCH_EXPIRED`. The same key and exact input reuse
the original receipt even after expiry or a policy change, subject to current
permission. A different input under that key returns `IDEMPOTENCY_CONFLICT`.

Memory kind and scope cannot change through this operation. A broader scope
requires a separately authorized proposal. Imported authority/approval fields
are rejected; even a direct operator patch uses machine-edit provenance. Use
the [review commands](review.md) for explicit human confirmation. A rollback is
a new patch against the current head, preserving all earlier revisions.

## Ask a configured Runner to investigate

Configure a named [executor](configuration.md), then supply feedback JSON:

```json
{
  "invocation_id": "BOUND_INVOCATION_ID",
  "executor": "editor",
  "targets": [{"memory_id": "MEMORY_ID", "expected_revision": 42}],
  "feedback": "Check the platform claim against the build evidence and notes.rs.",
  "expires_at_ms": 1794000000000
}
```

```sh
akasha-otsuge editor run feedback.json --idempotency-key investigation-1
akasha-otsuge job show EDITOR_JOB_ID
akasha-otsuge job cancel EDITOR_JOB_ID
akasha-otsuge patch show EDITOR_JOB_ID
```

Use an existing verified invocation. Its identity, workspace and scope come from
the stored Caller binding; feedback cannot replace them. The job pins the
current execution configuration and adoption policy. Explicit target IDs let
the Runner expand those exact memories; original and code reads retain existing
read-root and workspace protections. Executor revocation and current permissions
are checked again before publication, including after restart.

The report distinguishes memory error, stale conditions, retrieval gap,
execution error, environment, changed requirements and insufficient evidence.
Only memory error/stale conditions may carry a patch. Its targets must have been
expanded, and every cited span must have been read by this investigation. An
incomplete/conflicting report forces review even in automatic mode. The daemon
also retains its own tool failures, truncated originals/context reads, workspace
changes and history-page gaps, plus child-investigation gaps and conflicts.
A Runner cannot omit these to gain automatic adoption. A later output-budget
failure does not erase a limitation already observed during the operation.
A report of
insufficient evidence opens review without inventing a revision; a non-memory
cause produces a report without changing memory.

Model work runs outside the writer transaction. A durable validated response
checkpoint permits restart to finish publication without rerunning the model;
patch receipt reuse prevents duplicate revisions after a crash during commit.
Investigation gaps survive the checkpoint. An older checkpoint without that
coverage record requires review for a pending patch; an already committed patch
still reuses its original immutable receipt.
The original execution deadline remains in force. An interrupted invocation
without that checkpoint fails as `RUNNER_INTERRUPTED` and requires an explicit
new investigation. Inspect both job status and patch audit: a committed effect
remains recorded even if a later job completion/deadline check fails.

Automatic activation follows [the existing policy](review.md); protected human
decisions/preferences/procedures require review. Current searches filter replaced
revisions immediately, before the derived index catches up. Pending candidates
preserve the previous active revision. Historical state and delivered receipt
bytes remain available under current access checks.

This patch protocol replaces drafts. Incident-triggered suspension, causal
outcome linking and notification delivery are tracked separately in #21–#23.

Controlled cases, crash recovery and a real native investigation are described
in [the acceptance record](benchmarks/editor-acceptance.md).
