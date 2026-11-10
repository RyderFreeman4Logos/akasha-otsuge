# Execution outcomes

The [MVP-021](spec/issues/MVP-021.md) protocol and wrapper were merged in
[PR #52](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/52).
Focused results and scope limits are recorded in
[the acceptance evidence](benchmarks/outcome-acceptance.md).

An outcome identifies one execution event. Several builds in one agent run have
different event IDs, workspace observations, artifact hashes and log sources.
Submitting the same event again returns its original commit receipt. Reusing its
ID with different contents is an idempotency conflict.

The daemon resolves the invocation from its stored Caller binding. It derives
the run identity and scope, validates each attached Consult job against that
invocation, and obtains memory revisions from the actual saved RecallReceipt.
Caller-supplied fields cannot replace scope, receipt membership or human approval.
Reading or retrying an outcome still requires current access.
Structured submissions report observations supplied by their caller; the daemon
does not independently rerun their command or verify their filesystem hashes.
The bundled wrapper gathers those observations locally. Neither entrypoint
provides cryptographic execution attestation.

| Relation | What the evidence establishes |
| --- | --- |
| `attached` | This outcome attaches the specified receipt and its recorded revisions. |
| `returned` | The receipt records a delivery acknowledgement; this does not prove use. |
| `cited_or_claimed_used` | The reporting source explicitly claims use of an attached revision. |
| `suspected` | The source proposes a causal association, with any supplied evidence. |
| `validated` | The source supplies an explicit validation report and its evidence; attribution remains attached to that source. |

The last three relations remain attributable reports. Validating source hashes
and receipt membership does not establish a causal claim's truth. No relation is
automatically upgraded, and no outcome operation changes a memory or creates an
incident merely because a command exits nonzero. Explicit configured
[investigation signals](incidents.md) can admit policy-controlled handling.

The command wrapper records argv, exit status or signal, time, and workspace
observations before and after execution. Git evidence includes the base commit,
index digest, dirty diff digest and a digest covering tracked and nonignored
untracked contents. These are observations, not a filesystem snapshot held for
the command's lifetime. Missing coverage and changes during execution must stay
visible. Artifact paths, byte counts and hashes distinguish repeated builds;
recording a hash does not preserve an overwritten artifact's bytes.
Expected artifacts that are missing or cannot be safely read appear in
`execution.artifact_gaps`; they do not discard the command's failure record.

Log previews are bounded. Full retained bytes use the existing immutable Capture
archive, with exact event/revision/hash references. A truncated preview retains
its complete source locator. If source capture itself is incomplete, the outcome
must say so; it cannot call the retained prefix a complete log.

Every log and log page includes the immutable Capture `coverage` and effective
`completeness`. A structured log reference may report `complete`, `partial`
or `unknown`; omission means unknown. Metadata-only, unavailable and visible-text
sources always remain partial, even when the caller claims complete. An
available-payload source keeps the caller's completeness report. Thus an empty
retained payload does not imply an actually empty complete log. Preview truncation
is independent of source completeness. The wrapper reports complete only for each
fully retained part of its observed output. Legacy records recover coverage from
their pinned Capture event and keep unspecified completeness unknown.

## Command wrapper

Create a JSON plan with `event_id`, the verified Caller `invocation_id`, and an
optional `artifacts` array of relative regular-file paths. An optional `incident`
field carries an explicit validated investigation signal. For example:

```json
{"event_id":"build-1","invocation_id":"stored-invocation-id","artifacts":["target/app"]}
```

Run `akasha-otsuge outcome run plan.json -- COMMAND ARGS...`. It uses the bound
workspace and currently completed Consult receipt. Standard output is an outcome
JSON envelope even when the child fails. The exit status is the child's code,
128 plus a terminating signal, or 127 when the executable cannot start. Command
stdout/stderr are retained separately; they are not mixed into that envelope.

`outcome show EVENT_ID` returns the record. `outcome log EVENT_ID INDEX --start
BYTE_OFFSET` returns at most 32 KiB as lossless `raw_hex`, its exact source hash
and a `next_start` cursor. A log index names one contiguous stdout/stderr part.
Parts are captured in stream order, up to 512 KiB each and 64 parts in total.
These bounds are explicit failures, never silent capture truncation.

The wrapper creates an exclusive private marker under the daemon's data
directory before execution. Repeating that event ID refuses to execute, including
after an interrupted or failed archival attempt. This is local replay prevention,
not a distributed execution guarantee. Failure retains the execution observation
and raw spool for diagnosis; it never retries the command. Successful publication
removes the owned spool and leaves a compact receipt. A final immutable
`event.json`, when present after a transport failure, can be submitted with
`outcome submit`; an ambiguous earlier execution must be investigated before
choosing a new event ID.

Git and file hashes are streamed. Symlinks, submodules, unreadable or changing
files and unavailable Git metadata produce explicit workspace coverage gaps.
Ignored build outputs are covered only when explicitly listed as artifacts.
The wrapper observes the direct child and its output files; it cannot certify
that escaped descendants have finished producing output.
