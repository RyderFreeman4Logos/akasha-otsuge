# Review inbox and operator calibration

Review state is independent of memory lifecycle and notification acknowledgement.
Opening or dismissing an investigation does not activate or retire any memory.
New candidates and delegated conclusions needing review enter the inbox atomically
with their proposal. Tool-backed facts without assumptions need no automatic
queue entry. Use `review open` to investigate an existing memory, including a
candidate created before this inbox was introduced.

## Policy and applicability

```toml
[review]
mode = "policy"
```

| Mode | Source-checked proposal | Human authority |
| --- | --- | --- |
| `manual` | Remains a candidate | Requires exact operator approval |
| `policy` (default) | Tool-backed facts may be delegated active; other proposals remain candidates | Never inferred from model output |
| `automatic` | May be delegated active under the domain restrictions | Never inferred from model output |

The [adoption rules](domain.md) still restrict tentative evidence and hypotheses.
Automatic changes cannot replace adopted human decisions, preferences or procedures.
A pending successor leaves the previous adopted version in use. Only explicit
approval of that exact successor replaces it. A local exception uses a separate
memory with narrower scope. Retrieval returns the applicable human rules before
ordinary similarity results, with the more specific exception first; other
projects retain the global rule. Conditions remain evidence requirements, not
predicates that the daemon has automatically proven.

Extraction and Auto-Grill pin the review mode when a job is admitted. After
`config reload`, new operations use the new mode; in-flight jobs keep their
accepted generation. Legacy extraction jobs without a stored mode remain manual.
Policy reload does not retroactively adopt every pending candidate. A new
extraction operation can re-evaluate an exact pending candidate under a changed
mode, while preserving reviewed and already adopted records.

## Inspect and track

Use a JSON scope containing your actual daemon principal, for example
`{"user":"uid:1001","project":"alpha","branch_or_version":null,"session":null}`.

```sh
akasha-otsuge review list --scope /absolute/scope.json
akasha-otsuge review list --scope /absolute/scope.json --after REVIEW_ID
akasha-otsuge review show REVIEW_ID
akasha-otsuge review show REVIEW_ID --at COMMIT
akasha-otsuge review open /absolute/target.json --idempotency-key investigate-1
akasha-otsuge review state /absolute/update.json --idempotency-key investigate-2
```

Replace `REVIEW_ID` with the returned 64-character ID. `list` examines at most
32 records per page, includes records within the requested scope and returns
`next` when more remain. Follow `next` even when the filtered page is empty.
It is a current inbox, not a fixed-point export; `show --at` provides immutable
historical state. All reads, including historical reads, check current permission.

`target.json` binds the latest memory revision:

```json
{"memory_id":"the-memory-id","expected_revision":123,"reason":"Check this observation"}
```

`update.json` binds the review item's revision, not the memory revision:

```json
{"id":"RETURNED_64_CHARACTER_ID","expected_revision":124,"state":"investigating","reason":"Checking applicability"}
```

States are `open`, `investigating`, `resolved` and `dismissed`. Open/investigating
can move to another state; resolved/dismissed must reopen before another change.
Repeating a state is invalid. State changes alone never confer approval; `ack`
is not a review state. Reasons must be nonblank and at most 4 KiB.

## Approve, edit or reject an exact version

Create `decision.json` from the displayed review item:

```json
{"id":"RETURNED_64_CHARACTER_ID","expected_revision":124,"action":"approve","draft":null,"reason":"The original evidence supports this scoped claim"}
```

`draft: null` keeps the proposal. To edit, copy the complete `proposal.record.draft`
from `show`, change the intended fields, and place it in `draft`. The kind and
scope of an existing logical memory cannot change. References must still resolve
to exact original evidence. The original proposal and generator provenance remain
immutable; `show` returns both proposal and result, preserving their difference
and the operator's reason. Approval binds the exact edited semantic hash.

```sh
akasha-otsuge review preview /absolute/decision.json --idempotency-key approval-1
akasha-otsuge review apply /absolute/decision.json --idempotency-key approval-1
```

Run `apply` in a terminal. It displays escaped JSON with the complete before/after,
action, reason and configuration generation, then asks for `confirm HASH`. A
decline makes no change. There is no `--yes` or caller-provided approval boolean.
The daemon checks the peer UID, executable identity and controlling terminal;
the running CLI must be the same executable as the daemon. Generic IPC and a
noninteractive CLI cannot submit operator approval. This verifies the local
operator interface; it is not proof of physical human presence or isolation from
arbitrary code running as the same OS user. Workers must not receive trusted
review ingress or access to that operator interface.

Use `action: "reject"` with `draft: null` to reject the displayed version.
Rejecting a pending candidate keeps the previous adopted version; rejecting the
latest active version retires it. A new request against a stale memory or review
revision is refused. If permissions are revoked or configuration changes while
the prompt is open, confirmation cannot commit the old preview. Obtain a fresh
preview after resolving the change.

The review source, memory/adopted pointers, resolved queue item, audit, receipt
and outbox commit together. Crash recovery sees all of them or none. Repeating
the same decision and idempotency key returns its original receipt, including
after restart or reload; current permission is still required. Reusing the key
for changed content fails. Automated later revisions never inherit this approval.

Existing [domain bounds](domain.md#bounds-and-verification) apply. The serialized
decision source must fit 64 KiB and each stored record, including the before/after
audit, must fit 128 KiB. An oversized apply fails without partial changes. The CLI
does not support batch approval or incident deduplication; incident workflows and
notification delivery have separate Issues.

The [MVP-019 evidence](benchmarks/results/mvp019/README.md) records real daemon/CLI
flows driven by a synthetic PTY operator, controlled extractor responses and four
SIGKILL recovery phases. These fixtures do not claim a physical human action or
an authenticated model evaluation.
