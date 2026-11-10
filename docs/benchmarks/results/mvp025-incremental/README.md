# Neutral incremental import candidate evidence

This leaf extends the existing public archive import command to exact semantic
history prefixes. It keeps one complete native staging database, retains
ordinary-open/restore/import admission through publication, and uses a durable
old/new identity marker plus Linux atomic directory exchange. It does not merge
local branches or apply changed mempal snapshots.

The initial RED reproduces rejection of a longer common history and ordinary
open/restore bypass of the import lock. The recovery-order RED separately proves
that deleting displaced old data before validating the replacement native
identity is unsafe. Both causal logs are retained unchanged.

Final relevant focused selectors pass 60 Rust cases: archive 23, storage 17,
storage crash 11, daemon runtime 7, and actual daemon integration 2. Four child
tests are intentionally ignored in ordinary enumeration and explicitly launched
by their supervisors. The incremental supervisor covers nine publication
phases with SIGKILL and three live admission refusals at each phase. Additional
killed cases cover local writes, foreign stages, malformed recovery markers and
native-identity corruption. Runtime tests check newly imported jobs remain inert
across two starts. Clippy, formatting, plan/reference checks and diff checks pass.

Focused-r3 remains a nonzero aggregate: its core selectors passed, but the new
runtime fixture had intentionally revoked its source grant and failed before
incremental import. Focused-r4 repairs only that setup through the real grant API
and runs the runtime, actual daemon integration and Clippy checks. Focused-r1's
chmod fault similarly stopped at the new early admission boundary; the successor
uses an actual directory-fsync EIO interposer and retains both false-success
assertions. These fixture failures are not product GREEN or causal product RED.

The manifest binds the final candidate source/test bytes and every receipt.
Admission heads are the preceding docs-only commit; these are dirty-candidate
focused checks, not an exact-HEAD canonical gate. The complete canonical,
independent whole-range review and hosted CI/publication remain separate pending
steps. SIGKILL and injected EIO do not establish power-loss, real ENOSPC, a
50 GB corpus, 100-client memory limits or new provider authentication.

Issue #25 remains open for changed legacy snapshots and its whole migration
contract. The prior merged mempal reader's final evidence is retained in
[its own archive](../mvp025-mempal-final/README.md).
