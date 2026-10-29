# Active work and complete remaining checklist

Updated 2026-10-29 UTC after PR #42 merged at `d49be08`. Live GitHub audit: **16 open
Issues, 12 completed Issues**. #12 is waiting for authenticated Claude acceptance in draft PR #41. #14 is the
active independent leaf; its sole prerequisite #11 is complete. The complete
queue below preserves every Issue and dependency.
Each linked Issue's full acceptance criteria remain authoritative; this checklist
does not narrow them. Close an Issue only after evidence, review, gates and merge.

## Session operating rules

- Load `/home/obj/.hermes/skills/software-development/wd/SKILL.md` before every
  non-CSA task expected to exceed 90 seconds, including after compaction.
- Resolve this session's active provider TTL afresh for each arm; keep one shared
  parent relay. No cache-hit claim without telemetry.
- Launch long executable tasks with `nohup`, detached stdin and durable logs,
  PID/start-time, admission and terminal receipts. Keep one completion listener.
- At check-ins verify exact process identity, durable progress, original campaign
  age and a bounded next action. Reconcile terminal work before any resume;
  never reset an existing operation deadline or replay ambiguous effects.
- Keep one database writer. Reuse the 42 verified compressed source shards and
  existing writable SSD; stream the 84 GB expansion without retaining it.
- Keep `target` and `.spike-work` at the corresponding
  `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/`
  paths, reached through checkout symlinks. Do not recreate large artifact roots
  on the home filesystem. Remove owned obsolete build/probe/diagnostic outputs
  after their consumers finish; preserve compact receipts and reusable corpus.
- At a requested restart barrier, let native agents and non-durable jobs end
  naturally. Do not launch commits, long gates or new native agents; no handoff
  file unless requested. Resume only after the user confirms the restart.

## Restart review and process improvement

#1/#2 are merged and closed. #3's repeated full-scale failures held up all later
Issues. The final fixed 3,840-second concurrent case has now passed; its final
commit is 392298, distinct from the build interval's ending commit 370032.
The prior pause was intentional: measured work and reviewers ended naturally,
while follow-up validation, archive, final ADR/review and merge remained undone.

The six follow-up phases, final archive verification, exact-HEAD gate, independent
review and Linux CI passed; PR #31 is merged and #3 closed as completed. Reuse unchanged passing receipts, preserve each
historical binary's identity and retain all failures. Consolidate the remaining
checklist instead of appending stale running snapshots. Then implement one Issue
at a time in the specification's dependency order, with focused regressions and
one final exact-HEAD gate/review per candidate. No speculative parallel writers
or repeated full-corpus acceptance on unrelated intermediate changes.

The 2026-10-29 audit finds 16 open Issues (#12, #14–#28). #4–#11 and #13 are closed
with merged PRs #32–#38/#40/#42; no verified implementation waits for closure. #7's
review found an anchor race and its sibling; both have causal regressions, and
the final candidate passed its own gate, independent R3 review and CI. Finish
one dependency leaf through publication before merging the next. During a fixed
long acceptance, independent preparation may use one isolated SSD checkout and
the shared build cache, without changing the frozen source or release executable.
Keep one build writer and remove the temporary checkout after publication. Keep
passing unchanged full-data receipts, rather than rerunning them for each leaf.

#10's final exact-HEAD gate and hosted CI passed before the intentional restart.
After resume, the unchanged receipts were collected, independent R2 review
passed with zero actionable findings, and the PR/Issue were closed promptly.
The cold hosted build took about38 minutes; the independent CI-cache PR #39
merged before #11. CI40717707611 actually restored its1.94GB cache and ran the
full gate in about144 seconds. This one run proves restoration, not a general
benchmark. #11's exact gate and fresh whole-candidate R2 review passed; PR #40
merged and #11 closed. Collect terminal review/CI results promptly instead of
letting completed work wait through the next implementation phase.
Real host adapters remain #12–#15, not mock completion claims.

## MVP-003 completed acceptance

The unchanged [storage contract](../benchmarks/storage-contract.md),
[specification](../spec/issues/MVP-003.md) and
[evidence matrix](../benchmarks/storage-evidence-matrix.md) define acceptance.
The [decision record](../benchmarks/storage-decision.md) and immutable archives
retain exact commands, identities, original failures and scope limits.

- [x] Freeze both allowed candidates, licenses, exact source pins, manifest,
  oracle and thresholds. Helix fails three retained full-data low-memory recovery
  OOM runs; only the specified SurrealKV 1.0.0/LanceDB 0.40.0 fallback is evaluated.
- [x] Import and verify 50,000,002,783 unique UTF-8 bytes, 11,428,831 originals,
  1,000,000 real 1536-dimensional vectors and 100,000 deterministic changes.
  Historical verification has 5,163 assertions at its frozen commit point.
- [x] Verify complete lexical/vector coverage, 64 full scoped queries and 1,195
  authority-checked candidates; 32 ANN references cover all million vectors,
  with mean recall@20 0.9125. Freeze 3,840 distributed mixed queries and exclude
  their IDs from mutation.
- [x] Pass both 1,800-second/100-client file-cache-cold profiles, write burst and
  overload. Low RSS P99/max 458,305,536/493,187,072 B; high
  709,939,200/763,740,160 B. Their original native identities remain explicit.
- [x] Verify full backlog drain, high-to-low reopen and direct-low interrupted
  build recovery within the original deadlines. Production reload remains #8/#24.
- [x] Verify immutable history, permissions, real corrections/rescopes/retractions
  and restored bodies/vectors: 16 full-corpus candidates, 64 revisions, 755 native
  commands. Retain native crash, atomicity, derived-corruption, authority/WAL/VLog
  corruption, real kernel ENOSPC and configuration fixtures separately.
- [x] Reclaim full obsolete versions and inactive/partial generations through
  public operations; before/after witnesses preserve history, receipts and query
  candidate sets. Preserve filesystem versus inode-allocation distinctions.
- [x] Repair shared cache ownership and single baseline/delta builder admission;
  keep their causal regressions and failed diagnostics. Bind generation identity
  to the original import manifest and check reserve before legacy binding writes.
  Source 3b3994a passes the exact-HEAD root gate and independent R3 review.
- [x] Bind the existing full database with 139 native commands and 67 identical
  before/after witnesses, preserving state/deadlines without reimport.
- [x] Pass the complete 3,840-second/100-client concurrent rebuild on source
  3b3994a/native 860c32fa: 108,641 completed requests, zero operation errors,
  82,587 distinct seed/workload commits and768 corpus corrections. Both search
  types overlap the full11,428,831-text/1,000,000-vector build. RSS P99/max
  509,325,312/553,222,144 B; no OOM. All latency/throughput/content gates pass.
  108,672 is the planned count, not the executed count.
- [x] Verify completed-run sampled disk bounds: 371 samples, total peak
  137,661,698,048 B and minimum free 397,265,436,672 B; no category cap violation.
  Ten-second inode-allocation samples are not an instantaneous quota or exclusive
  Btrfs extent measurement. Completed reuse, exact process/unit exits and scratch
  removal pass. Retain both earlier full concurrent OOM failures and unknown
  outcomes unchanged.
- [x] Finish six already-prepared phases: all acknowledgements, pre-switch
  witnesses, eight original-vector replays, generation5 catch-up, activation and
  post-switch witnesses. 323 acknowledgement batches verify82,587 receipts; all
  514 follow-up commands pass. Active5 and both watermarks equal392298; immutable
  history/current bodies, indexed receipts and incompatible-bundle refusals pass.
- [x] Archive the full run, manifest binding, follow-ups, frozen source, commands,
  hashes, resource samples and prior gate/review receipts. All2,116 archive
  members pass hash/readback verification; compressed archive size22,486,670B.
- [x] Reconcile the complete crash/resource/config matrix against immutable
  receipts. Distinguish experiment recovery from product daemon/worker behavior;
  do not rerun unchanged checks without a concrete missing acceptance case.
- [x] Publish the sole-backend ADR and unchanged measured operating envelope.
- [x] Final exact-HEAD gate and independent whole-candidate review PASS at
  `8baaccc`; Linux CI40437740462 passed, PR #31 merged as `13065fc`, and #3
  closed completed. Product Storage and all later requirements stay below.

## MVP-004 completed acceptance

- [x] Implement the pinned authority engine and atomic revision/head/receipt/outbox
  transaction; preserve original evidence and append-only revision history.
- [x] Enforce durable idempotency, optimistic revision checks and bounded requests.
- [x] Define and guard storage format/schema; refuse unsupported formats safely.
- [x] Verify before/during/after commit crashes and whole-batch restart recovery.
- [x] Implement checksummed native checkpoints and restore into a fresh directory;
  verify content hashes/revision identity and reject invalid backups.
- [x] Record storage layout, migration policy, dependency/runtime no-SQLite audit,
  focused results, canonical gate, independent review, CI, merge and Issue closure.
  Candidate `5654abf` passes the complete gate and independent R2 review (0 findings),
  including the deterministic cancelled-commit/checkpoint fix. CI40446328008 passed;
  PR #32 merged as `95ff8f5` and #4 closed completed.

## MVP-005 completed acceptance

- [x] Define bounded source, actor, provenance, scope, conditions, evidence-span,
  memory-kind, validity, adoption and review schemas on the real Storage.
- [x] Separate untrusted payloads from verified server ingress; reject forged
  authority fields and keep worker messages outside trusted human adoption.
- [x] Enforce source applicability and project/version/session boundaries;
  preserve global taste alongside local exceptions and distinguish technical facts.
- [x] Bind review to the exact semantic content hash; retain old revisions,
  drop inherited approval on edits, and preserve durable retry/conflict behavior.
- [x] Test explicit trusted decisions under one-time policy, tentative statements,
  manual candidates and delegated active conclusions without human relabeling.
- [x] Publish schema/permission rules and misleading-input fixtures; complete
  focused tests, exact-HEAD gate, independent review, CI, merge and Issue closure.
  Candidate `664e49c` passes 10 native domain fixtures, the complete canonical gate
  and independent R1 review (0 findings). CI40450661442 passed; PR #33 merged as
  `9b9f483` and #5 closed completed. Real host authentication remains #11–#15.

## MVP-006 completed acceptance

- [x] Add bounded native revision/as-of reads and immutable commit-time metadata;
  keep source/reality time separate from when the system committed a decision.
- [x] Persist a scope/adoption index in the same authority batch, with fixed-point
  state/history pagination, bound cursors, byte/scan limits and no event replay.
- [x] Preserve pending, rejected, active, superseded and retired history; later
  corrections or backdated validity must not rewrite an earlier known-state view.
- [x] Store actual prepared/returned output bytes and hashes, selected revisions,
  source refs, invocation binding, read/validation points, config/index/storage
  identities, coverage/truncation and explicit delivery phases in RecallReceipt.
- [x] Recheck current read permissions for historical results without changing
  history; keep current and historical retrieval modes distinct and honest.
- [x] Verify temporal/pagination/replay/corruption/restart fixtures on real Storage,
  then complete docs, exact-HEAD gate, independent review, CI, merge and closure.
  Candidate `e98d4cb` passes 17 Domain and 17 Storage fixtures, the canonical gate
  and independent whole-candidate R1 review (0 findings). CI40454932598 passed;
  PR #34 merged as `42035bd` and #6 closed completed. Actual transport is #8;
  real indexes and host binding remain their dependent Issues.

## MVP-007 completed acceptance

- [x] Define source identity, raw payload, normalized events and derived-summary
  references with session/entry/parent/branch/fork cutoff and visibility.
- [x] Archive exact original bytes before derived work, with bounded chunks and
  durable atomic event/checkpoint publication on the formal Storage.
- [x] Preserve distinct identical-text events, deduplicate stable replay, reject
  conflicting identity reuse, and link deltas/final without final duplication.
- [x] Handle incremental/offline reconcile, incomplete tails, file rotation and
  truncation; expose missing, pending and unavailable coverage without guessing.
- [x] Keep compaction originals and branch inheritance cutoffs; verify late/out-of-
  order events, repeated resume/fork/compact and crash recovery with real fixtures.
- [x] Provide the reusable reconcile operation and executable component command,
  document its production CLI/IPC seam in #8 and actual host parsers in #12–#15.
- [x] Pass focused capture/domain/storage and four capture SIGKILL points;
  verify the actual command's JSON, exits, reopen, backfill and FIFO refusal.
- [x] Fix independent R1's accepted-frame/checkpoint-anchor rewrite race; retain
  causal RED/GREEN, bounded boundary regressions and release-command readback.
- [x] Sweep the saved-cursor sibling: preserve validated anchors on resume and
  verify both real reconcile boundaries and append-only control with ordinary
  temporary-file fixtures. R2 interruption remains no final review verdict.
- [x] Candidate `41a984d` passes its complete 23.181-second gate and independent
  R3 source review (0 findings). CI40506461591 passed; PR #35 merged as
  `960320a`, and #7 closed completed.
  [The protocol](../capture.md) records coverage and known uncollectable items.

## MVP-008 completed acceptance

- [x] Implement private Linux Unix socket ownership/modes, peer UID validation,
  version/capability/size handshake and bounded requests with stable errors.
- [x] Provide thin CLI daemon start/status/stop/doctor and bounded explicit
  DAEMON_UNAVAILABLE behavior, with no TCP or MCP listener.
- [x] Persist accepted jobs, immutable configuration plans, leases, bounded retry
  and cancellation state. Real SIGKILL/restart preserves all256 original/event
  receipts and no-op replay; queue/cancel/pause/overload and graceful restart pass.
  Durable recovery fixtures retain exhausted attempts and the original deadline.
- [x] Wire validate/reload/status/effective config and watcher to one transactional
  generation pipeline. Invalid-repeat/FIFO/rename/profile/large snapshot/pending
  restore pass; failed durable publication keeps active/pending/history unchanged.
  Validate/reload/startup share the original base paths when an override is removed.
- [x] Expose real memory/history/state/diff/RecallReceipt via IPC/CLI with current
  read permissions. Cursor/range parity, retirement/byte limits, revocation across
  restart and stdout delivery ACK pass; `/dev/full` does not mark delivery.
- [x] Verify another kernel UID denial with a working accessible control, no
  daemon-owned TCP listener, oversized/version-incompatible/busy refusal.
- [x] Audit disk use with dua; remove owned obsolete outputs, checksum-migrate
  target/.spike-work to the SSD mirror and verify all42 original shard hashes.
- [x] Complete installed release executable/API parity through the first canonical
  gate at `71090cf`; repeat for the repaired final candidate below.
- [x] Repair the four R1 review classes: collect mutation/scheduler owners on every
  shutdown path, activate requested paths on explicit restart, check live capacity
  at claim, and reject success after the last physical batch crosses its deadline.
  Causal RED/GREEN receipts and bounded lifecycle/restart regressions are retained.
- [x] Candidate `7552d97` passes its complete installed gate in78.23s:88 Rust
  tests,8 Python tests and9 installed daemon cases, including SIGTERM/SIGINT.
- [x] Independent whole-candidate R2 PASS,0 findings; CI40537161260 passed.
  PR #36 merged as `a3d278d`, same candidate tree, and #8 closed completed.

## MVP-009 completed acceptance

- [x] Reuse configured independent text/embedding/reranker routes; validate all
  outputs, bounded request/batch/response sizes, rate limits and explicit errors.
- [x] Add production CLI/IPC model jobs to the existing durable scheduler, pin
  accepted generations, keep provider clocks separate from queue/CLI clocks,
  support cancellation and never replay ambiguous interrupted external calls.
- [x] Cover success/malformed/slow/disconnect for all three endpoint contracts;
  virtual1699/1700 boundaries, sequential stages, cancellation and endpoint reload.
  Queue-clock and cancellation-trace defects have causal RED/GREEN receipts.
  Three near512KiB responses expose and repair aggregate archive overhead;
  the same requests now survive job reads and daemon restart.
- [x] Actual GB10 production-daemon compatibility passed at candidate `69853f1`:
  text returned OK, embedding4096 dimensions, reranker2 finite raw scores.
  The exact release0596d114 and routes are retained; this is not model quality.
- [x] R1 review exposed workflow renewal during config-restore admission. Causal
  executor RED/GREEN covers before/after expiry and already-expired execution;
  the original durable start/budget survives, and expired work sends no request.
  Provider admission also rechecks expired queue/workflow clocks before dispatch.
- [x] Repaired source `986a027` passes its82.535s canonical gate:98 Rust tests,
  8 Python tests and15 installed daemon cases. Whole-candidate R2 review passes
  with0 actionable findings. Actual GB10 text/embedding4096/reranker pass on the
  exact release6b6cc71; prior release receipts retain their own identities.
- [x] Historical release0596d114 passed the nohup production-daemon1605.029s
  success and1700s provider timeout, one physical send each; all owned processes
  ended and scratch was removed. This predates the R1 workflow repair.
- [x] Repaired release6b6cc71 passes its own real-wall run in1702.594s:1605.030s
  success and1700.001s timeout, one physical send each. Original job/workflow
  start and2010000ms budget match; owned processes and scratch are gone.
- [x] Archive148 original receipts/logs/reports (228200B), verify every hash,
  and document examples, exact identities and limits in
  [the acceptance report](../providers-acceptance.md).
- [x] Final candidate `6c23e1a` passes its 63.238-second canonical gate and
  independent R3 review with 0 findings. CI40560732248 passed; PR #37 merged
  as `a09d9d6`, and #9 closed completed. Exact gate and review are published in
  [the final PR receipt](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/37#issuecomment-6450384881).

## MVP-010 completed acceptance

All five independent R1 findings are repaired with causal regressions. The final
candidate `e27dc53` passes its exact-HEAD gate, fresh R2 review and CI. Its21 public
retrieval fixtures and five native index controls support the published acceptance;
PR #38 merged as `d566101` and #10 is completed. The historical gate at `987d369`
does not attest those repairs.

- [x] Implement real on-disk ICU keyword search and exact receipt replay/reopen.
- [x] Implement actual vector/embedding and reranker paths, model/dimension
  generation identity and explicit keyword fallback when models are offline.
- [x] Return applicable ancestor rules separately, preserve live scope grants,
  and test UTF-8 original ranges and truthful character-budget truncation.
- [x] Bound configuration, status permissions, source-span/output budgets and
  reranker documents; verify public IPC capability declarations.
- [x] Validate retired/corrected/current-validity and permission races at final
  output publication, including stale physical indexes and daemon restart.
- [x] Verify mixed Chinese/English, code identifiers, versions and conflicting
  global/local rules against a fixed retrieval quality baseline.
- [x] Persist separate contiguous lexical/vector prefixes, explicit skipped or
  superseded coverage, failed model-call diagnostics and crash-safe replay.
- [x] Use incremental native index updates and one bounded shared cache owner;
  prove queries continue while indexing waits and budget changes drain owners.
- [x] Preserve old-generation reads until safe new-generation activation and
  bind receipts to the actual model/index identity; reject incompatible state.
- [x] Keep the original workflow budget across every model stage and validate
  immutable sync targets; expose ongoing progress and coverage limits honestly.
- [x] Cover progressive reads of retained capture originals as required, without
  widening grants or pretending Memory-only indexes cover all capture content.
- [x] R1: Recheck live sync authority at every record read, provider admission,
  progress publication and activation; test a held embedding across grant shrink.
- [x] R2: Preserve the owner-wide permission required by archived index metadata
  across receipt show/replay and job show; retain narrow receipt access.
- [x] R3: Install both protoc and its standard definitions in CI and document them.
  Repaired-candidate hosted CI40581193846 passed.
- [x] R4: Persist zero/overflow cosine-vector failure diagnostics and the original
  prefix; verify identical diagnostic readback after restart.
- [x] R5: Admit a legal maximum-size wrapped Memory in the internal rule scan;
  retain public byte limits and bounded, honest search output.
- [x] Publish search/show documentation, quality and withdrawal fixtures, focused
  results, final exact-HEAD gate/review, CI, merge and Issue closure. Then #11–#28.

## MVP-011 completed acceptance

- [x] Versioned bounded JSONL envelopes, request/sequence correlation, capabilities,
  structured externally constructible errors, cancellation and terminal results.
- [x] Immutable runtime invocation binding before work; concurrent identities and
  missing/ambiguous/session-only/turn-specific capability refusal tests.
- [x] ContextSource with captured lineage/fork cutoffs, explicit gaps, authorized
  handles and current live read permissions; bounded workspace reads and hashes.
- [x] AgentRunner input/events/result and read-only tool policy; pinned generation,
  deadlines and shared start/depth/read/output budgets; immediate child refusal
  when no execution slot is available, including simultaneous parent delegates.
- [x] Out-of-crate adapter contract tests across all16 Caller/Runner combinations,
  including source failures, protocol errors and permission boundary failures.
- [x] Freeze at least 60 sourced reference intent tasks in #11 before any #16
  research-prompt tuning, with oracles, critical cases, at least10 confusing intent
  pairs, counting rules, fixed reference profile and commit/hash verification.
  Freeze commit `7356d17f0a373ade3520e6e1a1249fc52e17ba8d`, manifest SHA-256
  `6b6dffd7882dc703e393686d327f8aaae6becad1f5566e761b8cae8f4fad6ac7`.
  The gate pins this original baseline and refuses coordinated resealing.
- [x] R1 finding 1: retain the earlier Runner timeout after both Runner/Workflow
  deadlines expire, preserving cancellation and poisoned-state rejection.
- [x] R1 finding 2: preserve v1 and freeze a separate S04 permission-denial erratum,
  retaining the real revocation barrier, all60 cases and critical scoring.
  Erratum manifest `09bbd47c0080af5b9982438f6e75a1af40c8d57e7eb394d36c93767dab82db16`.
  Erratum freeze commit `f0dcef99bdc6aaae9635aa77054df525b688f5aa`.
- [x] Publish protocol/trait/binding/permission documentation and evidence, pass
  exact-HEAD gate and independent review, then merge and close #11. Candidate
  `639e991` passed146 Rust,17 Python and36 installed daemon cases in140.200s;
  R2 whole-candidate review passed with0 actionable findings. CI40717707611
  passed; PR #40 merged as `19efca1`. Full originals are published in the PR.
  The four real
  host integrations remain #12–#15; full quality/combination acceptance remains #27.

## MVP-012 draft PR #41 acceptance and remaining work

- [x] Verify the installed Claude2.1.311 executable against its official manifest;
  validate a native plugin and execute a non-model native metadata command with
  isolated config, unchanged user files, no other host CLI and no MCP server.
  The native TUI also executed that local command in a controlling PTY and exited
  normally, without model requests. This does not establish model acceptance.
- [x] Add the daemon-independent bounded CLI version probe and native transcript
  import through the existing durable capture cursor. The first slice preserves
  root-message parent links, visible text/tool results and partial-tail recovery;
  private thinking is excluded and a user role remains unverified authority.
  Focused regression:77 passed/5 existing ignored; all-target Clippy passed.
  Native child/fork/compaction coverage and live caller registration remain below.
- [x] Implement the smallest native plugin and Rust bridge for actual caller
  identity, parent/child registration, per-call turn/fence and immutable binding.
  Re-read identity after resume/clear; unknown child identity must refuse.
  The private root registration/binding bridge now passes durable retry/restart,
  identity replacement, missing-tip, unknown-field and live read-root withdrawal
  regressions; all-target Clippy passed. Actual native hooks now pass startup and
  resume, fork and native child binding plus13 durable backfill jobs against local
  synthetic SSE. The child registry preserves parent identity and rejects
  unregistered identities, changed parents and mismatched native source frames.
  Re-registration preserves call identity; a delayed transcript flush is retried
  within the original bound. Native sync waits for its persisted target event
  within the original job deadline. Native lifecycle reasons are retained;
  fork-parent session IDs are not exposed by the supported hook and remain
  unknown. Root tokens cannot claim child identity.
- [x] Capture original prompts, visible outputs and tool results; normalize native
  transcripts with bounded resume/backfill, partial-tail/gap handling, exact
  parent/fork/compaction lineage and no private-thinking extraction or false approval.
  Capture and binding now pass7 regressions and all-target Clippy,
  replaying the actual2.1.311 transcript. Attachments retain rendered visible
  text when present; native attachments without rendered bodies retain identity
  and an explicit omission marker. The fixture preserves that native absence.
  Summary flags remain recorded as session information. Missing original
  ancestry stays explicitly pending. Ordinary native children retain their own
  transcript ancestry and their registered runtime parent separately. Native
  fork replay retains copied originals and excludes later parent/sibling events;
  the unexposed fork-parent ID remains an explicit coverage limit.
- [ ] Implement a standalone Claude Runner using existing lawful CLI auth,
  structured terminal events plus exit status, bounded output, cancellation and
  original deadlines. Disable built-in/unrelated tools and enforce shared Rust
  read capabilities; verify denied writes, network and out-of-scope reads.
  The native process/read-bridge phase is implemented:23 focused tests and
  all-target Clippy pass. Native2.1.311 loads the actual plugin and completes
  list/read/final-output through real CapturedContext under a local SSE fixture;
  initial model input contains no original. Replayed native fixtures cover
  authentication, retries, tool pairing and incomplete/unsafe output. Native
  Bash/Read/WebFetch/ToolSearch denials, bounded output, original deadlines and
  process-group cancellation have separate evidence. This phase does not prove
  authenticated model acceptance or live Caller identity. Fixture provenance and
  reproduction are in `tests/fixtures/claude/2.1.311/README.md`.
- [x] Provide install/uninstall/rollback that preserves unrelated hooks and
  configuration; expose actual version, capability and coverage in doctor.
  The owned session plugin uses explicit native `--plugin-dir` loading. A new
  directory allows upgrade/rollback without overwriting the previous plugin.
  Integrity/extra-file/symlink guards and preserved native settings pass the CLI
  test; actual2.1.311 validates the generated plugin, captures a turn through its
  unmodified bridge, verifies doctor and uninstalls successfully. Runtime model
  authentication and loaded-session self-test are explicitly not asserted.
  Actual TUI type generation is supported: owned files remain hash-verified,
  generated type/config files and user edits within them are preserved on removal.
- [ ] Prove real TUI and Runner behavior with the other three CLIs absent:
  concurrent sessions, resume, compact, interrupted backfill, tool results,
  cancellation and denied operations, with traceable inputs/final output.
  Controlled local fixtures do not substitute for authenticated model acceptance.
  The unmodified installed plugin now captures a real controlling-PTY turn with
  native input/output UUIDs and cursor hashes, then passes doctor/uninstall.
  Permission metadata, duration events and nonpersistent local-command UUIDs
  have causal native evidence and regressions. A fixture-tool execution failure
  remains unclassified; richer diagnostics were added before the next passing run.
  Subsequent native checks pass concurrent sessions, TUI resume, manual compact
  and a turn after compact, retaining input/output UUIDs and original ancestry.
  A real daemon restart during an unfinished backfill preserves its deadline;
  the same job succeeds once the original arrives. Cancellation remains terminal.
  Authenticated model acceptance still needs an available lawful Claude login or
  provider configuration; the existing request for that input remains pending.
- [ ] Retain sanitized native fixtures and failures, official API/version evidence,
  reproduction commands and limitations; pass exact-candidate gate, independent
  review and CI, then merge and close #12. Continue every queue entry below.
  Candidate `0aede7c` passed the full local gate and exact-candidate native Caller
  and Runner protocol checks; draft PR #41 is published. Independent review found
  three defects that must pass new regressions, a fresh gate and re-review:
  - [x] Match the native read-tool schema to the shared 16 KiB reader limit.
    The actual native schema assertion fails before the fix and passes after;
    bridge tests accept 16,384 bytes and reject 16,385 bytes.
  - [x] Make uninstall recoverable after partial failure and preserve replacements.
    Permission-failure retry, detached-tree edits/replacements, resumed cleanup
    and preserved native configuration pass four unit and two real CLI tests.
  - [x] Bound Runner/probe reap waits while retaining cleanup ownership.
    The pending-reap regression fails with an unbounded wait and passes with
    a non-renewable one-second grace. Child/workspace/lease ownership survives
    caller drop; 17 focused tests and all-target Clippy pass.
  - [x] Diagnose and repair fixture startup before the fresh candidate gate.
    Gate2 at `bfc301b` failed before cancellation at executor startup. A later
    native spawn diagnostic reproduced `ETXTBSY` (OS26); script creation now
    occurs in an isolated writer process. A distinct procfs `ESRCH` observation
    is treated as an exited process. All33 focused cases, Clippy and20 parallel
    rounds pass; both earlier failures remain retained.
  - [x] R2: Reject changed native workspaces before binding root or child calls.
    The production JavaScript hook's child and grandchild regression fails on the
    old implementation. The repaired hook keeps the immutable session workspace;
    a cwd change requires a new native session. Root/child/grandchild and fresh
    session controls run in the shared gate with Node.js. Candidate `62ef371`
    passed its full gate, native checks and CI40776089474, but R2 found this
    binding defect; those passing checks do not approve the repair candidate.

Candidate `5424ead` passed the full159.958-second gate, fresh native Caller/Runner
checks, independent R3 review (zero actionable findings) and CI40778944541. The
[full review](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/41#issuecomment-6482322686)
records all four repaired findings. Its clean SSD checkout was removed after the
remote SHA was verified; branch and draft PR #41 retain the source. This section
records that draft implementation, which is not yet merged into main. Real Claude
authentication is the outstanding external prerequisite; #12 stays open.

The four original TUI onboarding probes remain setup failures. A distinct probe
used a controlling PTY and task-only trusted workspace configuration, ran the
local native command through the actual TUI, and exited successfully. User files
were unchanged. Real model/compact/cancellation acceptance is still required.

Host preparation: Claude Code2.1.311, Codex0.164.0, Hermes0.22.5+1914 and Pi0.78.0
are executable. Claude's broken link was repaired through mise onto SSD; the
native digest matches the official manifest. Non-billable auth status reports
unauthenticated. Its startup config migration was restored byte-for-byte; future
startup may migrate again. #12 still needs real host/model acceptance. PATH/version
checks do not prove host protocol capabilities or an authenticated Runner.

## MVP-013 completed acceptance

- [x] Pin native Codex0.164.0 schemas, hook fields and tool policy to the installed
  executable and official tagged sources. The native executable SHA-256 is
  `b93f6811d3f653f0fb83be4050d857fb9abbb111dfa2ce160563327daff7f306`.
- [x] Capture bounded native ordinal chains and original public messages/tools,
  preserve compaction originals and fork cutoffs, suppress duplicate replay and
  resume interrupted backfill. Missing history stays observable. Controlled
  native fixtures include a separate-process resume after fork; private reasoning
  and binary bodies remain outside explicit `visible_text` coverage.
- [x] Bind registered root session/turn/call/workspace to an immutable call-ordinal
  fence before tool execution. Reject ambiguous, explicit child or unknown native
  identity; never infer a child from a parent session. Cover concurrent roots,
  permission changes, restart, bounded catch-up and replay. Daemon registration
  and capture use the existing durable scheduler. The shared test-only empty
  readiness-marker race was fixed in `8aec4ac` without changing product timeouts.
- [x] Implement the standalone native app-server Runner with four scoped context
  tools, bounded frames/output, original deadlines, structured final/exit checks
  and retained process-group/lease cleanup. Fourteen ordinary Codex tests include
  19 injected failure scenarios; two explicit native tests are ignored by default.
  Capability negotiation advertises the actual four context operations.
- [x] Provide opt-in single-file install/undo and free version/capability doctor.
  Actual hooks bind outside the native sandbox and prepare a private per-call
  result for its offline reader. No credentials enter that file. Concurrent
  calls share no current-context file; installation conflicts preserve user data.
  The launcher explicitly exposes `functions` directly because nested Code Mode
  calls lack a provable rollout identity. No native configuration file is edited.
- [x] Verify actual authenticated Runner and two simultaneous authenticated TUIs,
  with other agent CLIs absent from PATH. Both TUIs have controlling terminals,
  distinct session/call/fence identities, isolated synthetic context and zero exit.
  Controlled native tests separately force denied writes, networking, outside
  reads, patch/MCP tools and unissued context handles. They prove the actual native
  tool boundary, while authenticated runs prove configured-model operation.
  [Sanitized acceptance](../adapters/codex-0.164.0/README.md) records exact binary,
  receipt and source-base hashes plus reproduction steps. Failed probes remain
  preserved. The successful TUI fixture allocated about4.9MiB, measured without
  following symlinks, and was removed after all consumers ended.
- [x] Run the canonical full gate on final clean candidate `329bba1`, then obtain
  independent full-range review. Gate3 passes165 Rust/17 Python tests plus40
  installed-daemon cases; R1 review passes with zero actionable findings.
  Historical gate1 timing failures remain unclassified. Gate2's private-TMPDIR
  UID fixture error has a separate causal RED/GREEN repair, preserving all checks.
- [x] [Hosted CI40801384231](https://github.com/RyderFreeman4Logos/akasha-otsuge/actions/runs/40801384231)
  passes at `329bba1`; PR #42 merged as `d49be08`, and #13 closed completed.
  Source worktree removed after all consumers ended; shared SSD target retained.

## MVP-014 acceptance and remaining work

- [ ] Qualify the installed Hermes version against official plugin hooks,
  canonical session/export and stream-json interfaces. Record mode/version limits
  and actual capabilities; keep doctor free of model calls.
- [ ] Parse one session per native JSONL export, retaining stable message UIDs,
  visible text/tool records, inactive compaction originals, parent/child lineage
  and interrupted-round backfill. Never tail obsolete session JSONL or invent
  human-input answers absent from native telemetry. Size guards, unsupported
  records, source changes and export failures must remain observable.
- [ ] Bind native session/task/turn/call identity and the owning profile before
  tool execution; persist an immutable fence, preserve retry identity, enforce
  current grants and prove simultaneous sessions/profiles cannot cross contexts.
- [ ] Ship the minimal native plugin and Rust bridge, opt-in install/undo and
  durable background capture. Verify hooks across real CLI/TUI and interruption,
  rather than assuming gateway-only or successful-turn callbacks cover all work.
- [ ] Implement independent one-shot stream-json Runner with profile selection,
  four scoped context tools, final-result plus exit-status checks, bounded output,
  original deadlines and process-group cancellation/cleanup. Disable unrelated
  native/MCP tools and verify writes, network and outside reads are denied.
- [ ] Run real authenticated CLI/TUI Caller and Runner with the other three
  CLIs absent; test at least two profiles, pre-compaction originals, native child
  sources and interrupted recovery. Retain actual export fixtures, source/binary
  hashes and sanitized receipts; distinguish controlled fixtures from model runs.
- [ ] Publish support/install/recovery documentation; pass final canonical gate,
  independent full-candidate review and hosted CI. Merge and close #14 with
  evidence, remove its owned scratch/worktree, then proceed to #15.

One source writer works in the SSD mirror and reuses the existing shared target.
#12 is deferred only for its recorded external authentication prerequisite;
#14–#15 have their prerequisite #11 complete. No second feature writer is active.

## Complete Issue queue

- [x] [#1 [MVP-001] 建立 Rust/Linux 项目骨架与可执行开发门槛](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/1) — dependencies: none.
- [x] [#2 [MVP-002] 版本化配置、热重载与 deadline 配置契约](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/2) — dependencies: #1.
- [x] [#3 [MVP-003] 非 SQLite 存储 spike 与唯一 backend 决策](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/3) — dependencies: #1, #2.
- [x] [#4 [MVP-004] 实现唯一正式 Storage、原子提交与恢复基础](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/4) — dependencies: #3.
- [x] [#5 [MVP-005] 定义并执行来源、authority、scope 与 taste 数据语义](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/5) — dependencies: #4.
- [x] [#6 [MVP-006] 持久历史视图、as-of 查询与实际 recall receipt](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/6) — dependencies: #4, #5.
- [x] [#7 [MVP-007] 采集归一化、原文归档及分支/压缩恢复协议](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/7) — dependencies: #4, #6.
- [x] [#8 [MVP-008] CLI、私有 IPC daemon 与持久后台作业](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/8) — dependencies: #2, #4, #7.
- [x] [#9 [MVP-009] 可配置本地模型客户端与 1700 秒全链长测](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/9) — dependencies: #2, #8.
- [x] [#10 [MVP-010] 磁盘检索、混合召回、重排与有预算的原文读取](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/10) — dependencies: #5, #6, #9.
- [x] [#11 [MVP-011] CallerAdapter/ContextSource 与 AgentRunner 公共契约](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/11) — dependencies: #7, #8, #10.
- [ ] [#12 [MVP-012] Claude Code CallerAdapter 与 AgentRunner 实接](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/12) — dependencies: #11.
- [x] [#13 [MVP-013] Codex CallerAdapter 与 AgentRunner 实接](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/13) — dependencies: #11.
- [ ] [#14 [MVP-014] Hermes CallerAdapter 与 AgentRunner 实接](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/14) — dependencies: #11.
- [ ] [#15 [MVP-015] pi CallerAdapter 与 AgentRunner 实接](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/15) — dependencies: #11.
- [ ] [#16 [MVP-016] consult 可选短 hint、渐进调研与子孙共享预算](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/16) — dependencies: #12, #13, #14, #15.
- [ ] [#17 [MVP-017] 异步提取候选、来源引用与可重建索引作业](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/17) — dependencies: #5, #7, #9, #10, #16.
- [ ] [#18 [MVP-018] 单一或组合 Runner 的 Auto-Grill 工作流](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/18) — dependencies: #16, #17.
- [ ] [#19 [MVP-019] 生效策略、review 队列、人工校准与 taste 使用规则](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/19) — dependencies: #5, #6, #18.
- [ ] [#20 [MVP-020] 外部 Memory Editor、范围受限 patch 与并发修订](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/20) — dependencies: #16, #19.
- [ ] [#21 [MVP-021] Run、Git/工作区、build/test 与 incident 证据关联](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/21) — dependencies: #6, #11, #16.
- [ ] [#22 [MVP-022] 执行失败触发调查、自动修订或高优 review](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/22) — dependencies: #20, #21.
- [ ] [#23 [MVP-023] Notifier 插件、可用本地/stdio 实现与持久投递](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/23) — dependencies: #8, #19, #22.
- [ ] [#24 [MVP-024] 50 GB、100 客户端、内存预算与故障恢复压力验收](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/24) — dependencies: #4, #8, #10, #16, #17, #20, #23.
- [ ] [#25 [MVP-025] mempal 只读迁移、中立导出与往返验证](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/25) — dependencies: #4, #5, #6, #7, #19.
- [ ] [#26 [MVP-026] Linux 安装、升级、卸载与四宿主使用文档](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/26) — dependencies: #12, #13, #14, #15, #23, #24, #25.
- [ ] [#27 [MVP-027] 质量评测、主上下文开销与组合回归](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/27) — dependencies: #16, #18, #19, #20, #22, #24, #26.
- [ ] [#28 [MVP-028] 需求逐项终验、发布和完成证据归档](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/28) — dependencies: #1, #2, #3, #4, #5, #6, #7, #8, #9, #10, #11, #12, #13, #14, #15, #16, #17, #18, #19, #20, #21, #22, #23, #24, #25, #26, #27.

Every leaf retains the complete linked acceptance criteria. No local fixture
substitutes for the real four-host, provider-duration or release acceptance.
