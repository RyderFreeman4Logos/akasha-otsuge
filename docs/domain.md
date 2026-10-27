# Provenance, scope and adoption

MVP-005 adds `domain::Domain` above the sole native Storage. The Rust types in
`src/domain/schema.rs` define domain schema 1. Untrusted `SourceDraft`,
`MemoryDraft`, `MemoryWrite` and `ReviewRequest` reject unknown fields, including
forged authors, authority and approval markers. Every nested input structure is
strict. No confidence number substitutes for provenance or applicability.

## The trusted boundary

`Ingress` is a server context with private fields and no Deserialize implementation.
Agent, tool and unverified constructors cannot create human provenance. Agent
contexts retain runner, model and prompt version. A reported `user` role remains
source data; source text, environment flags and worker consensus grant no authority.

The two `from_verified_*_channel` constructors are trusted Rust integration
boundaries, **not authenticators**. The host/control adapter must first verify
the real input channel, actor, binding, permitted scope and action. A review
context additionally fixes memory ID, proposal revision, semantic hash and
approve/reject action. The same target is persisted with the original review
source; a different recomputed hash cannot reuse that grant. Workers must never
receive these contexts or call their constructors through an IPC payload.

These component fixtures do not authenticate a real host. #8/#11–#15 integrate
the real control/host channels and live permissions; #19 owns the operational
review queue and configuration integration. Same-UID arbitrary shell/native
library access is not a human-isolation boundary. Raw Storage and Domain reads
are trusted server APIs, not authorized public IPC endpoints.

## Stored evidence and semantic identity

`SourceRecord` retains the supplied event identity, source actor/origin, channel
binding, scope, event time, observed time, exact UTF-8 text and its SHA-256.
Source keys use a hash of event identity, not text, so identical statements at
different event identities remain distinct. Source records are immutable.
Full host event normalization, branch/fork metadata and large-event chunking
belong to #7. Reported event/observed times do not replace the commit sequence.

A source span binds event ID, immutable Storage revision, whole-text hash and
a nonempty UTF-8 byte range. Missing revisions, changed hashes, invalid ranges
and split code points are refused before a write. Each source's scope must contain
the resulting memory scope. Even a broadly permitted worker cannot silently turn
project/session evidence into a global principle.

`MemoryRecord` retains kind (`decision`, `preference`, `fact`, `procedure`,
`hypothesis`), claim, rationale, conditions, assumptions, evidence spans, optional
reality validity window, original provenance, adoption, policy/config identity
and the actual authorization settings used. Event time, reality validity and
Storage commit order remain separate. [Time views](history.md) use durable
commit points and retain system-clock observations independently of these fields.

The semantic hash is SHA-256 over deterministic JSON of
`(1, memory_id, draft, provenance)`. It covers scope, conditions, assumptions,
all source references, validity and generator identity. An approval binds this
exact hash and proposal revision, reviewer, original review source, action and
reason. Review evidence is in `approval.source` in addition to `draft.source_spans`.
Editing a proposal can be approved, but the trusted grant must cover the edited
hash. Subsequent proposals have no inherited approval. Stored original proposals,
approvals and receipts remain immutable.

## Adoption rules

`Policy` is supplied by the trusted server with an explicit revision, configuration
generation, allowed scope, review mode and explicit-decision opt-in. The record
preserves these settings; reusing a request ID with changed policy settings is a
different canonical request. It is not a caller-provided authority declaration.

| Input and policy | Result |
| --- | --- |
| Agent/tool proposal, manual | Candidate; existing adopted version remains in use |
| Agent/tool proposal, automatic | Delegated active, subject to the restrictions below |
| Policy/selective mode, fact supported entirely by tool evidence | Delegated active |
| Other selective proposals or unverified ingress | Candidate |
| Tentative evidence used for a decision/preference, or a hypothesis | Candidate |
| Explicit verified human statement with adoption enabled | Human-confirmed direct decision event |
| Exact trusted approve operation | Human-confirmed active revision; original generator provenance remains visible |
| Reject a pending/rejected candidate | Rejected revision; prior adopted decision stays active |
| Reject the latest active revision | Retired revision; adopted pointer becomes inactive |

Direct statement adoption copies the **complete** original statement, retaining
its scope. It cannot cut a clause, paraphrase the claim, invent a rationale,
conditions, assumptions or validity window. Structured extraction can produce a
separate proposal; a trusted review can approve its exact structured content.
This path requires no second per-statement confirmation once the server's
explicit-decision adoption policy is enabled. Intent is a verified-channel
assertion; this module does not guess intent from keywords or model confidence.

Automatic proposals cannot replace a human-adopted decision or normative
preference merely because a technical test failed. That protection follows the
adopted revision across any number of pending proposals. Agent-generated
conclusions may otherwise be active under policy and retain delegated authority.
Model consensus never creates human confirmation. Reject operations bind the
unchanged displayed proposal hash; edits use a new proposal or an approve operation.

## Scope and current state

Scope contains user plus optional project, branch/version and session. A version
requires a project. `Scope::contains` preserves every declared constraint;
`more_specific_than` is a partial relationship, not a numeric global ranking.
Different projects or versions are incomparable. Conditions remain explicit
evidence requirements, not automatically proven predicates.

An existing logical memory cannot silently change kind or scope. A project/version
exception uses a separate logical memory and coexists with global taste. Within a
query's permitted context, its more specific applicable human decision can serve
as the local exception; unrelated contexts retain the global decision. Current
instructions and actual conflict/ranking workflows are integrated by #10/#19.

The latest revision and adopted revision are separate. `memory(id)` returns the
latest proposal/review; `active(id)` resolves the adopted pointer. An adopted
revision and its State pointer commit in one native transaction with the receipt
and outbox. Pending/rejected proposals update only the latest revision. Historical
relationships use Storage's immutable `previous` chain and the retained adoption
revision; no full database snapshot or original-event replay is involved.

Writes carry the exact expected latest revision. Validation reads that immutable
version, not a mutable current head, so an identical retry still returns its
original receipt after later changes and after restart. A new request against a
stale revision conflicts. A review targets the latest proposal; broader queue,
batch-review and current-head retraction workflows remain #19/#22 work.

## Bounds and verification

IDs are nonblank and at most 256 UTF-8 bytes. Source text is at most 64 KiB, claim
32 KiB, rationale 16 KiB, and each conditions/assumptions list has at most 32
entries of 4 KiB each. A draft has 1–32 source spans. Storage's **serialized**
128 KiB per-record and 1 MiB transaction bounds also apply, so heavily escaped
text may hit that earlier limit. #7 must chunk according to the encoded size.
Errors expose stable codes and do not print source bodies.

`tests/domain.rs` uses the real SurrealKV Storage in disposable private fixtures.
It covers forged fields and user labels, source scope ceilings and byte boundaries,
policy modes, tentative speech, Auto-Grill provenance, exact review grants,
edited hashes, repeated post-review proposals, local exceptions, rejection and
retirement, original receipt retries, conflicts and native close/reopen.

Development retained behavioral RED cases for ignored forged fields, truncated
direct statements, a recomputed hash reusing a generic review channel, a second
proposal overriding human taste, and rejection accidentally removing the old
adopted version. These are now focused regressions. Final exact-HEAD gate,
independent review, CI and merge receipts are recorded on
[Issue #5](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/5). Candidate
`664e49c` passed the full gate and independent R1 review (0 findings), Linux
CI40450661442 passed, PR #33 merged as `9b9f483`, and #5 closed completed on
2026-10-27. No real host/model test or
whole-product completion is claimed here.
