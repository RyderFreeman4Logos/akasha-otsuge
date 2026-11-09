# MVP-020 editor acceptance

The source range begins at `460934961009ff41ae1b3def573e81dda4fa7d0e`.
Core atomic patches are committed in `159aad804c922b2cfd43c29e997321a1fdbfc3ed`;
native investigation/recovery in `9b3ce6cb794b5fd3ca14c6043df4da6ae7ca5251`.
The [archive manifest](results/mvp020/manifest.json) hashes 67 compact files
(178,601 bytes before the manifest). No corpus or build directory is archived.

| Evidence | Result and limit |
| --- | --- |
| `mvp020-core-focused-r2` | Four patch tests, 20 domain tests, nine review tests, four extraction tests, formatting and Clippy passed. One opt-in extraction test skipped. |
| `mvp020-native-workflow-green-r2` | Configured controlled Codex Runner read source/code, produced a patch, preserved provenance and reused the completed job. |
| `mvp020-native-boundaries-r2` | Eight editor integration tests passed; genuine Runner test skipped. Two library recovery tests failed in binding setup; later chained checks did not run. |
| `mvp020-recovery-resume-r1` | After supplying the synthetic binding's missing executable: two recovery tests, 31 Consult tests and Clippy passed. A crash after response checkpoint or atomic patch commit resumes without Runner replay or duplicate revision. Missing-checkpoint interruption requires explicit retry. |
| `mvp020-native-actual-r1`, `mvp020-native-diagnostic-r2` | Pi 1.4.0 could not connect to localhost:48317. The diagnostic retained only error events (`Connection error.`). These are failed native attempts, not model investigation evidence. |
| `mvp020-native-codex-r1` | Actual authenticated Codex 0.164.0 completed the unchanged public editor workflow against synthetic evidence. The small test-only host-selection patch is archived with its admission digest. |

The [controlled regression source](../../tests/support/daemon_editor.rs) covers
exact revisions, atomic batches, concurrent writers, stale/expired requests,
idempotency, scope and evidence rejection, forged authority, live revocation,
pinned policy, non-memory causes, original-byte preservation and old receipt
replay. Controlled host processes do not establish native authentication.

The real run used the existing Codex profile (configured `gpt-6.1-sol`, OpenAI),
one Runner start, and the retained binary SHA-256
`b93f6811d3f653f0fb83be4050d857fb9abbb111dfa2ce160563327daff7f306`.
The profile selection is recorded separately from observed adapter usage;
the receipt reports 69,672 tokens and nine budgeted reads, not an independent
provider billing measurement. Caller identity and the fixture were controlled.

The Runner expanded `editable` revision 2, read the immutable Linux-only original
and `notes.rs`, and changed “all platforms” to “Linux; other platforms were not
tested.” The exact scope remained unchanged; patch commit 102 has `memory_edit`
provenance and no human approval. The report identified the overclaim conflict
and an unavailable search index; it therefore produced a review candidate even
though the admitted mode was automatic. This proves conservative policy handling,
not automatic activation of this particular native result. Automatic activation
and manual policy are independently covered by controlled tests.

All three native attempts removed their exact scratch directories after owned
processes ended. Pi settings and the existing Codex configuration were unchanged.
The [Codex receipt](results/mvp020/mvp020-native-codexr1-result.json) contains the
before/after drafts, input, citations, job trace and patch audit. Original failures
and their source/dirty-tree identities remain in the archive; a later success does
not replace them. Early dirty-tree receipts do not by themselves reconstruct all
historical test source bytes.

Independent R2 review of `77a78c6` found two introduced defects despite that
commit's canonical PASS: editor adoption ignored daemon-observed investigation
gaps, and the CLI rejected protocol-valid patches above 16 KiB. The
[repair archive](results/mvp020-review-fixes/manifest.json) preserves that FAIL,
the earlier INCOMPLETE reviewer-runtime attempt, gate diagnostics and regressions.
Three controlled investigations reproduced unsafe activation when the model
omitted a tool failure, original truncation or workspace change. The SIGKILL
recovery and legacy-checkpoint cases reproduced the same policy failure. The
public CLI reproduced rejection of a valid 20 KiB patch.

The repair shares Consult's gap union with Editor, checkpoints those gaps and
requires review when a legacy checkpoint lacks coverage. It uses the protocol's
64 KiB limit for the patch CLI while preserving other commands' 16 KiB limits.
`mvp020-review-green-r1` passed 12 editor integration tests, three recovery tests,
32 Consult-related tests and Clippy. The explicit native-editor and crash-child
tests were skipped in that focused batch. This is controlled repair evidence;
the authenticated Codex run above remains historical evidence for the unchanged
native protocol, rather than a new authenticated run on the repair.

The next candidate, `d9d1e4a`, passed its canonical gate but R3 review found a
remaining sibling: a successful history page could report `HISTORY_GAP` while
the invocation itself was `CompleteToFence`. Research did not retain that page
warning. The [coverage archive](results/mvp020-coverage-fix/manifest.json) preserves
R3 FAIL, its exact gate, the complete response-class inventory and new regressions.
Before repair, five controlled Editor cases wrongly activated corrections:
missing-parent list/search, truncated context text, and child gaps/conflicts
omitted by the parent. Ordinary Consult returned `found` for a truncated read.
A boundary test also reproduced loss of the output-budget warning; a subsequent
combined-error control reproduced loss of an earlier read denial.

The shared response boundary now retains successful-response warnings before
output charging, keeps both earlier and later errors, and preserves their original
return behavior. Validated conclusions retain child limitations before returning
to the parent. The existing bounded union and checkpoint carry these observations
into Editor policy and ordinary Consult. `mvp020-coverage-green-r3` passed
17 editor integrations, four recovery/boundary tests, 32 Consult-related tests and
Clippy. The preceding compile diagnostic is retained and is not behavioral evidence.
SIGKILL tests cover publication of the new gap labels from a controlled checkpoint;
fresh investigation tests independently verify production of those observations.

Earlier capture/IPC suite-wait failures remain unclassified; a stale shared-target
base/candidate comparison is explicitly invalid. The unchanged prior candidate
passed its one canonical rerun. Neither that PASS nor the later repair identifies
the historical timeout cause.

The complete canonical gate, independent review and hosted CI are recorded on
the publication PR for its final immutable candidate. This document alone does
not assert those later stages passed. Incident suspension and notification remain
the separate #21–#23 scope; no Claude authentication is inferred from Codex.
