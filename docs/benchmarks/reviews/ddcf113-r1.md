VERDICT: FAIL

# Issue #3 / PR #31 independent native storage review, round 1

R1 is a HIGH correctness regression that blocks the model-isolation acceptance claim. R2 is a LOW whitespace issue. This verdict concerns the reviewed selectable fallback source. It is not full benchmark acceptance, a CSA tier verdict, a merge approval, or authorization to close issue #3.

## Frozen identity and review boundary

- Repository: `/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge`.
- Branch: `spike/mvp-003-storage`.
- Frozen HEAD: `ddcf1136ea3d2a6b13e104065642d161c2e58a64`.
- Frozen tree: `94e88837842057663ace89751bce01e222766954`.
- Base, merge-base, observed `origin/main`: `349431a350e9e4ae3364b126c4d2a7133dd37d22`.
- Attribution range: `349431a350e9e4ae3364b126c4d2a7133dd37d22...ddcf1136ea3d2a6b13e104065642d161c2e58a64`.
- Supplied native SHA-256: `8f0b2b2f1a6890a78b0efba421176e95e906a4d03607fd5efab8292c00942d00`.

The independent reviewer used `native-code-review` and its applicable scope, frozen-evidence, durable-state, materialized-index, verdict, and finalization references. Source was read from frozen Git blobs. No source, database, branch, or commit was changed. Only the two expressly allowed review artifacts under `.git/akasha-evidence` were written; the parent retained sole implementation/database ownership and the shared WD relay. The task explicitly prohibited Cargo, dynamic tests, benchmarks, full corpus/database reads, large-archive extraction, and further agents. This is therefore a source-only review with supplied execution provenance, not independently repeated execution.

## Findings

### R1 — REGRESSION / HIGH (P1): a fresh generation can relabel original corpus vectors

**Reachable failure.** After importing the prepared `text-embedding-3-large` corpus, call `begin` for a previously unused generation ID with a syntactically valid bundle naming another model or fingerprint. Native admission accepts it. Backfill copies the original vectors without re-embedding; build can mark them ready; query and delta output checks compare with the newly stored self-label. This violates `docs/benchmarks/storage-contract.md:120-121`, which requires that “model/dimension identities never mix.” The dimension is constrained to 1536, but equal dimension does not establish equal model semantics. The bundle's chunker label is likewise not source-bound.

**Source proof.**

1. `spikes/data/prepare_corpus.py:95-105` consumes `text-embedding-3-large-1536-embedding`. Lines 181-186 put `embedding_model`, `dimensions`, publisher date, chunk bytes, and input revisions into the manifest.
2. `spikes/fallback/src/load.rs:30-33` deserializes only `struct Manifest { corpus: Expected }`. Persisted import `State` at lines 44-56 keeps a manifest hash and counters, but no semantic model/fingerprint/chunker identity.
3. `spikes/fallback/src/search.rs:119-135` validates bundle shape only. In `Begin`, lines 1081-1090 optionally compare manifest bytes with the stored import SHA. Lines 1093-1119 reject different labels only for an existing generation; the fresh-generation branch directly persists the caller's `bundle` with the corpus SHA. Byte lineage alone does not compare the bundle's semantics.
4. `search.rs:637-647` reads original corpus records, and lines 680-686 append `batch(&rows, true, state.text_layout)` to the new vector table. No re-embedding occurs. The query comparisons at `search.rs:980-981` and `search/changes.rs:855-865` use the same stored label.

These runtime files are additions against the authoritative base (`git diff --name-status` confirmed `A`). R1 is therefore a regression for the requested branch range, not an attribution to the latest retirement commit. The failure is established by admission-to-copy-to-query control flow; no dynamic reproduction was run under the no-execution assignment.

**Same-root apply/replay and evidence surfaces.**

- `spikes/fallback/src/search/changes.rs:130-152` checks `v.model != bundle.model`, fingerprint, body hash, dimension, finite values, and nonzero values. Completion/pending hashes bind the supplied delta correctly, but the generation baseline is not source-bound. A delta labeled as the new model can share a generation with baseline vectors from the original model. No separate completion-hash or watermark-replay defect was found.
- `spikes/fallback/src/search/workload_backend.rs:100-105` validates ready/source/active and returns `base.bundle`; lines 158-167 use that bundle with fixture vectors. `spikes/query_fixture.rs:98-103` fixes the fixture model to `text-embedding-3-large` without establishing that it matches the returned generation bundle.
- `spikes/fallback/check_corpus_lifecycle.py:144-148` takes `bundle = base['bundle']`; lines 199-202 wrap `original['embedding']` in that bundle's model/fingerprint before apply. A mislabeled generation can therefore obtain self-consistent replay evidence.
- `spikes/fallback/check_real_search.py:112-133` checks source/neighbor and vector-byte consistency while using the status bundle. `spikes/helix/run_generation.py:125-134` freezes manifest bytes. These checks do not substitute for semantic admission.

**Coverage gap.** `check_search.py:104` changes the label on an existing generation; line 118 changes only a query bundle. `check_lifecycle.py:100-114` creates a second generation with a schema-only change, then tests query/output disagreement with its stored label. None admits a fresh generation with a source-incompatible model/fingerprint/chunker.

**Required closure.** Bind model, fingerprint, dimension, and chunker to trusted metadata from the byte-bound source manifest; reject incompatible fresh generations and validate existing generations at a common read boundary used by query/workload/apply/replay. Any legacy metadata binding must match the original import SHA. Preserve independent schema upgrades and existing deadlines; original vectors must not silently acquire a new model identity. Add a fresh-generation negative control, valid-generation positive control, and legacy/read/replay controls. Keep synthetic fixture provenance explicit. The parent's proposed minimum repair is consistent with this boundary but was not implemented or reviewed here; a new frozen-HEAD review is required.

The parent reports that the current full-data candidate uses the correct fixed bundle. R1 does not establish that its existing numeric measurements are wrong. It prevents treating those measurements as proof of the broader model-isolation requirement.

### R2 — REGRESSION / LOW: blank line at end of shared oracle

`spikes/oracle_model.rs:30` adds a blank line at EOF. The independently run read-only command `git diff --check 349431a350e9e4ae3364b126c4d2a7133dd37d22...ddcf1136ea3d2a6b13e104065642d161c2e58a64` returned exit 2 with `spikes/oracle_model.rs:30: new blank line at EOF.` Remove the extra blank line. This is patch hygiene, not another runtime blocker. The supplied root gate's unqualified `git diff --check` checks working changes, so its clean result does not contradict this committed-range result.

## Other traced invariants

These observations are source evidence, not independently executed acceptance results.

| Focus | Result and limit |
| --- | --- |
| Authority durability/history | `authority.rs` serializes writes and uses an Immediate transaction for immutable revision/head, receipt, outbox, sequence, and scope. Replay is checked before new mutation. Historical selection and current permission publication were traced. |
| Import/backfill recovery | Bounded record validation, exact stream hash/counters, committed-prefix resume, durable pending waves, native append count/version/tag reconciliation, and publication after both families were traced. They establish byte lineage, not R1's semantic identity. |
| Shared derived admission/deadlines | `search.rs:1023-1052` preserves the original deadline, releases the authority writer before waiting on shared derived admission, includes the wait in the deadline, and refreshes generation state after acquiring it. No inverse writer/derived lock order was found. |
| Activation | `search/changes.rs:736` checks index coverage under derived admission, then takes the authority writer, requires both watermarks at the current authority point with no pending work, and atomically publishes the active pointer/receipt. |
| Incomplete retirement/pruning | `search/changes.rs:513,603` protect active ownership and drained coverage, reject pending delta work, persist retirement intent before deletion, and tolerate already-absent tables on resume. Incomplete inactive generation state and original deadline remain auditable. These standalone offline CLI operations rely on exclusive database process ownership. |
| Query filtering | `search/changes.rs:845` checks current permission, authoritative revision, scope, and activity before publication. Line 944 explicitly states `candidate_set_only`; truncation before filtering can reduce results and does not prove global ranking or recall. |
| Workload/controllers | Heavy search admission is separate from ordinary permits; original request/catch-up deadlines and declared result thresholds are retained. Controllers bind immutable inputs, durable phase state, and completed artifact hashes; observer attribution/cleanup were traced. No full-scale resource margin is inferred. |
| Fixtures | Import/authority/search/lifecycle/corpus/query/workload fixtures cover separate seams. `check_generation_workload.py:187` explicitly excludes full scale, 30-minute profiles, and vectorized candidate corrections. Preserve those exclusions in acceptance reporting. |

## Gate provenance and limits

| Inspected artifact | SHA-256 | Permitted inference |
| --- | --- | --- |
| `.git/akasha-evidence/partial-retirement-publication.json` | `fbba9ab805e95b94bd878fe5c9dc409d8a3c2ffd3ede03d30fe26028386792a7` | Supplied frozen-HEAD mapping to native SHA and root gate log; states 12 release units, Clippy, 223 lifecycle commands, 19 SIGKILL boundaries, 2 contenders, and implementing-agent self-review only. Remote/CI fields were not independently queried. |
| `.git/akasha-evidence/partial-retirement-root-gate-r1.log` | `55013ca11450ed0ec1c3180f1b0a26b906c1de3e53c8dd7c24ee9e651d3ac4cb` | Supplied terminal output reports plan validation, 6 Python tests, 4 CLI tests, 17 config tests, root Clippy/release and version success. The root workspace contains only `.`, and fallback is its own workspace; this is not a native fallback gate. |
| `docs/benchmarks/results/fallback-partial-retirement/summary.json` | `22e36df959d43458915924af113c40be046afe5694526eb843f850be85ee8e74` | Compact native fixture summary for SHA `8f0b2b2f1a6890a78b0efba421176e95e906a4d03607fd5efab8292c00942d00`; explicitly excludes full generation-4 reclamation, full concurrent resource acceptance, and sole backend decision. |
| `docs/benchmarks/results/fallback-derived-admission/summary.json` | `eac9d48dadbdd9af5540a0592e1284cae91cadb55a9ef0073cb2e056b75a2245` | Earlier diagnostic summary for native SHA `d5b97f674d803f49bfaf02a6be835f86604694f983acd91543ecce8d57344a05`, not execution of the newer binary; excludes full concurrent memory/performance and sole backend decision. |

The root recipe and manifests were inspected. These execution receipts were supplied frozen evidence, not rerun checks. Compact summaries' archive counts/digests were treated as metadata; archives were not unpacked, and their archived command logs were not exhaustively inspected. The full database/corpus and running full-data task were not opened. No benchmark or issue closure is asserted.

## Covered paths and exclusions

Read in full: fallback `src/{authority.rs,load.rs,search.rs,main.rs}`, `src/authority/oracle.rs`, `src/search/{changes.rs,workload_backend.rs}`; shared `spikes/{workload.rs,query_fixture.rs,oracle_model.rs}`; reused `spikes/helix/{run_case.py,run_generation.py}`; `spikes/measure.py`; `spikes/data/prepare_corpus.py`; fallback `{check_import.py,check_authority.py,check_search.py,check_lifecycle.py,check_corpus_lifecycle.py,check_real_search.py}`; Helix-directory `{test_workload_oracle.py,check_generation_workload.py}`; root/fallback Cargo manifests; `scripts/check.sh`; `docs/spec/issues/MVP-003.md`. Relevant storage-contract/spec sections and compact summary metadata were selectively inspected; whole-file hashing is not whole-document review.

Excluded: rejected Helix-native engine implementation, unrelated root application/config changes, other issues, dependency internals, raw archives, production/live deployment, physical power-loss durability, and full benchmark/resource acceptance. No PRE-EXISTING blocker is asserted. Prior self-reviews are context, not independent proof.

## Filesystem instructions and custody

Exact logical `AGENTS.md` and `CLAUDE.md` paths were checked with filesystem existence/symlink checks, not Git tracking. Both were absent and not symlinks in every directory below, so there are no instruction targets or line counts. Core runtime ancestor checks preceded source inspection; document/evidence descendants were also checked during finalization.

Ancestors: `/`, `/home`, `/home/obj`, `/home/obj/project`, `/home/obj/project/github`, `/home/obj/project/github/RyderFreeman4Logos`.

Relative to repository: `.`, `spikes`, `spikes/fallback`, `spikes/fallback/src`, `spikes/fallback/src/authority`, `spikes/fallback/src/search`, `spikes/helix`, `spikes/data`, `docs`, `docs/benchmarks`, `docs/benchmarks/results`, `docs/benchmarks/results/fallback-partial-retirement`, `docs/benchmarks/results/fallback-derived-admission`, `docs/spec`, `docs/spec/issues`, `scripts`, `.git`, `.git/akasha-evidence`.

At review start, HEAD/tree matched the freeze and porcelain was clean. During finalization the parent explicitly announced seven owned evidence/plan file changes below. They are excluded from the frozen source review and were hashed without reviewing their contents; no archive was unpacked. HEAD/tree/base remain frozen, and all 30 inspected working blobs still equal their frozen hashes. The companion progress JSON records exact instruction paths, all inspected blob Git IDs/SHA-256 values, dirty-file custody, and post-write attestation. Report SHA-256 is returned externally after final writing.

| Parent-owned changed path | SHA-256 at finalization |
| --- | --- |
| `docs/benchmarks/results/fallback-full-generation5-preparation/files.json` | `355c4c752a813eab59aa928d956d70c89a5b6672663e0249d8edb7a2745660f2` |
| `docs/benchmarks/results/fallback-full-generation5-preparation/raw.tar.gz` | `06f2c2881c4200282bcfd5dff90f8a8620f85a754bb703662398cbf1b009d7ad` |
| `docs/benchmarks/results/fallback-full-generation5-preparation/summary.json` | `37116c3ba3960e91a1c14288b1f8b02fdd05a7bb3ab3efaa3a5893029fa04fc3` |
| `docs/benchmarks/storage-decision.md` | `adf174baf9b46ebddf072855d36de73b276cc171b167ad6e8788e02acd875c9d` |
| `docs/benchmarks/storage-evidence-matrix.md` | `c680daf903a87e64d7b27ebac6c638217f3152a7dd6ee3fe763ee0cd0b58cbd3` |
| `docs/plan/progress.md` | `2e657d47496025ae4060da5fc8102094823cdcf6a6252af6bacaa519334fe05a` |
| `docs/plan/todo.md` | `4db9bc6b35c2d0ff8bcbcad88301c611b010eeb86943316def5ab100f862643f` |

VERDICT: FAIL
