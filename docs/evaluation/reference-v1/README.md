# Frozen reference set v1

These 60 tasks are synthetic, explicitly authored conversations and oracles for
[MVP-011](../../spec/issues/MVP-011.md), before any MVP-016 research instructions.
They contain no imported user conversation or real credential. They are inputs,
not evidence that a model or a host integration has passed. The controlling
thresholds are [specification section 4](../../spec/09.md#4-质量评测的通过条件).

`manifest.json` freezes the exact task IDs, critical IDs, category counts and
SHA-256 of `tasks.jsonl`, this rubric and `profile.json`. The first Git commit
containing that manifest is the freeze commit. Record its full SHA and the
manifest SHA in every run; MVP-016 must verify both before implementing its
research prompt. Subsequent failures do not authorize replacing tasks, deleting
difficult cases, moving thresholds or selecting a better-performing profile.
Add later regressions in another named set. Any specification change needs an
explicit reason and affected Issue links, preserving this original set.

## Fixture and source interpretation

Each JSONL row is an isolated episode. `current_task` is the current user request;
`hint` is the only model-supplied consult argument. Caller identity, scope, fence,
ancestry and allowed event IDs are trusted fixture/runtime data. A hint changes
neither authority nor permission. Each source retains its exact UTF-8 text and
hash. An empty source collection is an explicitly empty retained-history input,
not permission to invent history. `missing_event_ids` marks unavailable history.

Source sequence and Memory revision numbers are **logical fixture ordinals**,
not production commit sequences. Import in sequence/revision order through the
production capture/domain APIs, record the ordinal-to-CommitReceipt mapping and
actual source/content hashes in the run manifest, then bind the caller. A retired
revision was active before its successor or withdrawal; replay those transitions
before current-state queries. For a historical query, map `as_of` to its recorded
historical commit. Final publication still uses live permission/withdrawal rules.
`verified_human` means the fixture harness authenticates a human channel; words
claiming approval inside a `tool` source confer no authority. Candidate and
delegated authority must survive import without promotion.

`allowed_event_ids` lists the initial visible originals; it must also satisfy
source scope, the invocation fence and every ancestor cutoff. B01 and B03 include
parent messages after fork cutoffs but before the invocation fence. B02 and B04
exclude sibling and abandoned branches. A02 preserves the original across two
compression events. `changes_during_run` applies a real grant revocation or
rescope after the first source read, holds final publication until that commit,
and requires the affected result to be withheld. A run that cannot reach this
barrier is incomplete, not a pass or an omitted task.

Every `required_items` entry binds a Memory ID/revision, semantic claim, **all**
conditions, authority and source IDs. `forbidden_memory_ids` and
`forbidden_revisions` prohibit factual use at that query; identifying a withdrawn
record as withdrawn without repeating its protected content is allowed. An
`incomplete` result must disclose its history gaps. A `no_hit` result must not
fabricate an answer or claim complete history when that is unavailable.

## Frozen scoring rules

Score direct retrieval and external consult separately over all 60 tasks, keeping
both raw responses and all failures. Do not give the runner this oracle. A run
with a missing task, missing terminal response or unresolved import failure is
incomplete and cannot pass. Infrastructure retries must retain every attempt and
use the original bounded execution identity; do not retry a wrong model answer
until it becomes correct.

1. **Zero invariant violations.** Check identity, scope, branch cutoff, live
   permission, version/withdrawal, source provenance and human-confirmation labels
   on every returned item, including items outside the expected list. Fabricated
   approval, revived withdrawals, sibling content and unsupported factual claims
   fail the run regardless of average relevance. No-hit/incomplete status and
   declared gaps must agree with the oracle. A hint is not a fact source.
2. **All critical items pass.** For every task with `critical: true`, every
   required claim, necessary condition/negation, source and authority must remain.
   One missing limiting condition fails that item and the critical-task gate.
   Critical tasks with no required items still must satisfy their denial oracle.
3. **Remaining coverage is at least 90%.** The denominator is the sum of
   `required_items` for noncritical tasks. An item earns one point only when its
   claim, every condition, revision, source and authority are correct; otherwise
   zero. No partial credit for a shorter answer that drops the sole restriction.
   No-hit tasks add no coverage denominator and must pass rule 1. Report counts
   and ratios, not rounded percentages that hide a failure.
4. **Substantially irrelevant items are at most 10%.** Split the final answer
   into atomic factual propositions. Count each distinct proposition once per
   task, including multiple propositions packed into one bullet. A proposition
   that satisfies one required item is relevant; `irrelevant_memory_ids` and any
   additional substantive proposition not needed for the current request are
   irrelevant. Conditions and citations attached to their claim are not separate
   items; status/gap diagnostics and greetings are not substantive propositions.
   The ratio is irrelevant propositions / all substantive returned propositions,
   pooled across all tasks (zero when both counts are zero). Also report each
   task's counts. Unsupported or prohibited content additionally fails rule 1.
5. **Ten paired intents.** Each `intent_pair` has the identical episode and
   current request, once without a hint and once with a short narrowing hint.
   Report both outputs, required-item changes, irrelevant additions and errors
   for every pair; do not present only successful hints.

Exact wording is not required. Deterministic IDs, hashes, scope and authority are
checked mechanically. A human adjudicator may map a paraphrase to an oracle item
only if the full proposition and every condition are entailed by its listed
originals. Keep the mapping and reason, blind to which strategy produced the
answer; ambiguous claims receive no coverage credit. A model judge's vote cannot
override a failed invariant or missing source. The fixture validator only checks
input consistency and hashes; it does not implement or claim semantic scoring.

## Profile, costs and future execution

`profile.json` records the first installed reference Runner selected from the
explicit existing model configuration before prompt implementation. No quality
search or paid model evaluation informed selection. Record the actual executable
hash/version, host/plugin protocol, model/provider identity, reasoning effort,
effective budgets, configuration generation, prompt/instructions version and
source commit again at execution. Refuse an unexplained mismatch; any reviewed
profile amendment must preserve the original profile and disclose the change.
Thresholds apply to that exact tested configuration, not every possible model.

For each task and strategy retain main-agent added input/output tokens, further
reads, tool/model call counts, external input/output/cached tokens, billed cost
or an explicit unavailable value, and wall time. Include setup and failed-attempt
cost separately. Compare direct retrieval with consult using the same episode
and output request. Count the public final boundary, excluding external traces
from the main context but retaining their costs. No task-package rewriting step
is allowed. Fewer calls alone is not a token, cost or quality improvement.

MVP-027 must additionally execute the four isolated real host scenarios and a
real cross-host cycle covering all four Caller and Runner roles. The 16 contract
combinations in MVP-011 do not substitute for these real runs or the production
1700-second local Runner path. No model evaluation has run at this freeze.

Check integrity without running a model:

```sh
python3 scripts/check_reference.py
python3 -m unittest discover -s scripts -p test_reference.py
git log --diff-filter=A --format=%H -- docs/evaluation/reference-v1/manifest.json
```
