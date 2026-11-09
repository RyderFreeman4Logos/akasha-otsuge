# Outcome-driven investigations

An explicit signal on an [outcome](outcomes.md) can open a scoped investigation
and run the existing [Editor](editor.md). A nonzero exit alone creates no
incident. Sources report reproducible failures, user corrections, dependency
changes, expiry or direct contradictions; the daemon does not scan the entire
memory store or infer a cause from an exit code.

## Configure admission and authority

```toml
[incidents]
executor = "investigator"
triggers = ["reproducible_failure", "user_correction", "dependency_changed", "expired", "contradiction"]
deduplicate = true
escalate_after = 3
auto_repair = false
max_runner_starts = 2
max_tokens = 64000

[incidents.priorities]
expired = "normal"
user_correction = "urgent"

[executors.investigator]
kind = "codex"
binary = "/path/to/supported/codex"
native_home = "/path/to/existing/native/profile"
```

Use an installed, supported native profile and authorize its home/workspace in
`access.read_roots`. This example does not install a host or provide credentials.
By default there is no executor, no enabled trigger, and no automatic repair.
Signals with a disabled trigger remain ordinary immutable outcomes. An enabled
trigger without an executor still creates an open item for operator review.

The policy reloads for new signals. Each accepted group pins its executor,
configuration generation, start/token budgets and original queue/workflow clocks.
Current permissions, trigger enablement and executor revocation still apply.
Automatic adoption requires both the pinned and live `auto_repair`, sufficient
read evidence, and the existing review policy. Enabling automation later cannot
upgrade an accepted manual investigation; disabling it can require a candidate.
Protected human decisions/preferences retain their existing authority rules.

The shared start budget is 1–32; tokens are 1–10,000,000. Native reported tokens
include cached input and are not a currency estimate. Explicit fallback and
delegation use this same budget. The automatic workflow deadline includes the
slowest selected executor/fallback and the full start budget. Admission has a
256-item pending bound. An unavailable executor, exhausted admission capacity,
timeout or interrupted run leaves a visible item with its error.

## Report a signal

Add this optional field to an `outcome submit` event or `outcome run` JSON plan:

```json
{
  "incident": {
    "kind": "reproducible_failure",
    "key": "client-api-v2",
    "reason": "Compare the recorded compiler failure with the v2 API fact and current code.",
    "targets": [{"memory_id": "api-current", "expected_revision": 42}]
  }
}
```

This fragment needs the ordinary event/plan fields and actual memory revision.
The wrapper validates it before executing the command and carries it into the
outcome. It never reruns a command under an existing event ID. The signal is a
report, not human approval or proof that a memory caused the failure.

Targets are limited to eight distinct memory IDs and positive revisions. Every
target requires current permission and must stay within the bound Caller scope.
When omitted, targets come from attached receipt revisions. An empty set can
still investigate missing recall; it grants no patch targets. Oversized target
sets retain an open review without selecting an arbitrary subset. Historical
revisions can be investigated, but publication still checks the current head.

By default, the same principal, bound scope, source label, trigger, reported key
and exact target revision set form one group. Different invocations can join it.
Each distinct event increments the recurrence count and retains its own reference;
replaying the same event does neither. This is explicit grouping by a source's
reported key, not a semantic claim of a common root cause. `deduplicate=false`
creates one group per event while preserving event-level idempotency.

| Priority rule | Result |
| --- | --- |
| Explicit trigger override | Configured normal/high/urgent |
| Default user correction | urgent |
| Default reproducible failure or contradiction | high |
| Default dependency change or expiry | normal |
| Any critical-severity event | At least urgent |
| Count reaches `escalate_after` (1–1,000,000) | At least high |

Reasons are retained and a group's priority never decreases. Priorities describe
review urgency; they do not freeze unrelated work or change worker scheduling.
Repeated reports update an existing item, including a finished one, without
automatically starting another Runner or publishing another patch.

## Read evidence and handle the result

The investigator uses the same nine context/memory/code tools as Consult.
`context_describe.investigation` exposes handles for the admitted immutable
outcome and its logs. `context_read` pages their JSON documents at up to 16 KiB;
the Runner continues until `end == total_bytes`. Log documents preserve UTF-8
text or lossless hex, original source references, hashes and coverage. Read
fragments retain exact offsets and immutable source spans in the investigation
trace. Reading a preview is insufficient for automatic adoption.

Unread log ranges, partial/unknown source logs, missing execution observations
for reproducible failures, workspace/artifact gaps and all ordinary Editor
limitations remain explicit. Shared read/output limits and the 64-source bound
can stop a large investigation; retained outcome logs are not truncated to make
it pass. A model cannot omit observed limitations to obtain automatic authority.
Receipt attachment proves exposure eligibility, not use or causality.

Reports distinguish `memory_error`, `stale_conditions`, `retrieval_gap`,
`execution_error` (Agent misuse), `code_bug`, `environment`,
`requirements_changed` and `insufficient_evidence`. Only the first two may carry
a patch. A complete active repair resolves the incident. A fully evidenced
non-memory explanation can dismiss it. Missing recall, uncertainty, manual
candidates and failures remain open. All model explanations remain attributable
reports; the daemon validates their evidence/authority rather than judging truth.

The existing Editor checkpoint and patch receipt make recovery idempotent.
A crash between job admission and incident association reconnects the existing
job, even after its original queue deadline. Deadlines never restart. A run
interrupted without a validated response checkpoint fails with
`RUNNER_INTERRUPTED`; ambiguous model work is not automatically repeated.

## Inspect and act

```sh
akasha-otsuge incident list --scope scope.json
akasha-otsuge incident show INCIDENT_ID
akasha-otsuge job show EDITOR_JOB_ID
akasha-otsuge patch show EDITOR_JOB_ID
akasha-otsuge incident update update.json --idempotency-key operator-action-1
```

Lists and event references page 32 scanned entries; use each returned `next`
cursor with `--after`, including after an empty filtered page. Use a scope that
contains the incident's bound scope: an owner-level Caller produces an owner
incident even when its patch target is project-specific. Current permission is
checked for every read.

An update contains `id`, the displayed `expected_revision`, `state` and a
nonempty `reason`. States are `open`, `investigating`, `resolved` and `dismissed`.
A terminal state can reopen; replaying the same exact update reuses its receipt.
Stale concurrent updates return a revision conflict. Resolving/dismissing an
in-flight investigation revokes further execution. Reopening a finished item
does not replay its Runner; an operator can use a new explicit Editor request.

Incident state, memory adoption and notification acknowledgement are separate.
An incident update does not approve a candidate or send a notification. Persistent
notification delivery is the following MVP-023 scope. Focused and native evidence
is recorded in [the acceptance record](benchmarks/incident-acceptance.md).
