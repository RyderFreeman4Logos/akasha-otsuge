# Execution outcomes

Implementation in progress for [MVP-021](spec/issues/MVP-021.md). The structured
submit/show protocol passes its first persistence, permission and attribution
regression. Command execution, full log retrieval and final acceptance remain pending.

An outcome identifies one execution event. Several builds in one agent run have
different event IDs, workspace observations, artifact hashes and log sources.
Submitting the same event again returns its original commit receipt. Reusing its
ID with different contents is an idempotency conflict.

The daemon resolves the invocation from its stored Caller binding. It derives
the run identity and scope, validates each attached Consult job against that
invocation, and obtains memory revisions from the actual saved RecallReceipt.
Caller-supplied fields cannot replace scope, receipt membership or human approval.
Reading or retrying an outcome still requires current access.

| Relation | What the evidence establishes |
| --- | --- |
| `attached` | This outcome attaches the specified receipt and its recorded revisions. |
| `returned` | The receipt records a delivery acknowledgement; this does not prove use. |
| `cited_or_claimed_used` | The reporting source explicitly claims use of an attached revision. |
| `suspected` | The source proposes a causal association and provides evidence. |
| `validated` | The source supplies an explicit validation report and its evidence; attribution remains attached to that source. |

The last three relations remain attributable reports. Validating source hashes
and receipt membership does not establish a causal claim's truth. No relation is
automatically upgraded, and no outcome operation changes a memory or creates an
incident merely because a command exits nonzero. Investigation and policy-driven
handling belong to MVP-022.

The command wrapper records argv, exit status or signal, time, and workspace
observations before and after execution. Git evidence includes the base commit,
index digest, dirty diff digest and a digest covering tracked and nonignored
untracked contents. These are observations, not a filesystem snapshot held for
the command's lifetime. Missing coverage and changes during execution must stay
visible. Artifact paths, byte counts and hashes distinguish repeated builds;
recording a hash does not preserve an overwritten artifact's bytes.

Log previews are bounded. Full retained bytes use the existing immutable Capture
archive, with exact event/revision/hash references. A truncated preview retains
its complete source locator. If source capture itself is incomplete, the outcome
must say so; it cannot call the retained prefix a complete log.
