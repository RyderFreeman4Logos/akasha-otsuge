# Reference v1 erratum 1: whole-grant revocation

The active reference is the original 60-task [reference-v1](../reference-v1/)
plus this separately pinned erratum. All original files stay byte-identical to
freeze commit `7356d17f0a373ade3520e6e1a1249fc52e17ba8d`, whose manifest SHA-256 is
`6b6dffd7882dc703e393686d327f8aaae6becad1f5566e761b8cae8f4fad6ac7`.
This correction was made during MVP-011 independent review, before MVP-016
research instructions or any model evaluation. The later acceptance records must
name both manifests and this erratum's freeze commit.

`corrections.json` changes only S04's expected outcome. Its real
`revoke_read_grant` commit must still occur after the first protected source read
and before publication. The run must return a typed `error` with `READ_DENIED`,
no protected content (including the CEDAR marker) and no successful-result
RecallReceipt. A durable failed-job/error diagnostic is allowed; it must not be
presented as a successful recall receipt. Missing the barrier, a missing run, an
unrelated failure or a synthesized no-hit answer does not pass.

The original no-hit oracle was unsound: live `Domain::permission` denies the
revoked caller scope, and `Domain::prepare_recall` checks that same authority
even for an empty result. A permission failure cannot publish a successful empty
result in the revoked scope. This is an oracle correction, not a relaxation of
the production permission or publication checks.

All other S04 fields, its critical status, source bytes, caller scope/fence and
forbidden memory remain unchanged. A04 still expects `no_hit`: moving a memory
outside a retained caller grant can filter that memory without revoking the
caller's authority to publish a safe result.

This erratum supersedes only S04's result-status requirement in the original
scoring rules. The counts remain 60 tasks, 31 critical tasks, 10 paired intents,
20 critical required items and 36 noncritical required items. Critical S04 passes
only its explicit denial oracle. All other item, condition, source, relevance,
paired-intent, model/profile and threshold rules remain unchanged. Expected
permission errors are scored as such, not dropped from the denominator or
treated as infrastructure failures.

`scripts/check_reference.py` pins this manifest independently of its contents,
checks the original freeze first, applies only the declared oracle fields, and
validates the effective 60-task set. Consumers use `read_reference()` rather
than reading uncorrected task JSONL directly. New changes require another named
frozen artifact; neither v1 nor this erratum may be resealed.

Structural checks are not real model or publication acceptance. MVP-016/MVP-027
must exercise the real revocation barrier and publication path.
