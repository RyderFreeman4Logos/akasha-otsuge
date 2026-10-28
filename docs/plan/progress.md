# Development checkpoint — 2026-10-28 UTC

Repository: https://github.com/RyderFreeman4Logos/akasha-otsuge

The public Apache-2.0 repository contains all41 original specification pages and
28 linked Issues. The specification snapshots remain immutable. #1–#11 are merged
and completed; #12–#28 remain open at this checkpoint. The full acceptance queue
is in [todo.md](todo.md); each linked Issue retains its complete criteria.

## Current issue: MVP-012

Claude2.1.311 is installed through mise on SSD with an official-manifest-matching
binary. Its native plugin validator and a non-model metadata command pass in
isolated config; the command returns the actual native session ID and version.
User configuration remained unchanged. Native TUI onboarding probes exposed a
terminal-driving setup problem and retain their failures; TUI and authenticated
model acceptance remain open. The full remaining implementation/acceptance list
is in [todo.md](todo.md). The finished CI checkout is reused for #12 on SSD,
sharing the canonical build target rather than copying it.

## Completed contracts: MVP-011

The shared contracts, durable immutable binding, fenced context/workspace reads,
terminal results and global delegation budgets are implemented. Focused checks
pass20 Rust tests, including16 Caller/Runner role combinations through real
CapturedContext, plus9 reference validation tests and all-target Clippy. The
[protocol and permission boundaries](../adapters.md) and
[original development receipts](../benchmarks/results/mvp011/README.md) distinguish
these contracts from real host integration and final consult publication.

The60-task reference (31 critical tasks,10 paired intents) is frozen at
`7356d17f0a373ade3520e6e1a1249fc52e17ba8d`; manifest SHA-256 is
`6b6dffd7882dc703e393686d327f8aaae6becad1f5566e761b8cae8f4fad6ac7`.
The gate rejects coordinated resealing of the original inputs and manifest.
Independent R1 review found an overwritten timeout cause and S04's unsound
no-hit outcome after whole-grant revocation. Causal regressions now pass. The
separate [erratum](../evaluation/reference-v1-errata-1/README.md) changes S04 to
typed `READ_DENIED`, without protected content or a successful RecallReceipt;
v1 remains unchanged. Erratum manifest SHA-256 is
`09bbd47c0080af5b9982438f6e75a1af40c8d57e7eb394d36c93767dab82db16`.
Erratum freeze commit: `f0dcef99bdc6aaae9635aa77054df525b688f5aa`.
Reference profile Codex0.164.0 / gpt-6.1-sol / high was selected before research
instructions, with actual installed executable provenance. Model evaluation has
not run. Final candidate `639e991` passed its140.200-second canonical gate:
146 Rust tests,17 Python tests and36 installed-release daemon cases. Fresh
whole-candidate R2 review passed with0 findings, and CI40717707611 passed.
[PR #40](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/40) publishes the
complete original gate/review/CI logs, merged as `19efca1`, and closed #11.

The separate [CI-cache PR #39](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/39)
merged as `dc8e27cd4329dcbc41cdf37cf63a4eafcee548c2` after its exact local gate,
independent PASS/0-finding review and hosted CI40636923468. First miss/save is
confirmed. PR #40 then restored the1.94GB cache successfully and still ran the
whole gate, from15:37:49 to15:40:13 UTC. This verifies a real cross-run restore;
it is a single observed run rather than a general performance benchmark.

## Completed retrieval: MVP-010

Candidate `e27dc5391483e48dfcb268a9fc9a0daba828e7b4`, tree
`629d2a2e6a22d01032cff96c13755a92358db648`, passes the117.311-second exact-HEAD
canonical gate:126 ordinary Rust tests,8 Python tests and36 installed-release
daemon cases. The21 public retrieval cases and five native index controls cover
scope/current revisions, rules/conflicts, generation pinning, SIGKILL prefix
recovery, live permission shrink, archived diagnostics, budgets and progressive
original reads. Installed/release SHA-256 is
`37d2b9ebab808e20eff7b3e9533011aacb1c66a6605e373f53d7f302ed37b4f2`.

Independent R2 review passes with0 actionable findings and closes all five R1
findings. CI40581193846 passes on GitHub's synthetic merge of this head and base;
PR #38 merged as `d5661016c3bfb057f4b01799035ef84c45d8450c` with the same tree,
and #10 closed completed. [Final gate and review originals](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/38#issuecomment-6461008510)
retain raw logs, SHA-256 and environment. [The retrieval contract](../retrieval.md)
links the frozen quality baseline and all development/failure receipts. Product
scale and actual model quality remain #24/#27, not inferred from tiny fixtures.

## Completed providers: MVP-009

Independent provider clients and durable model jobs are implemented on
`feat/mvp-009-providers`, based on merge `a3d278d`. Repaired source `986a027`
passes its82.535s canonical gate:98 Rust tests,8 Python tests and15 installed
daemon cases. Independent whole-candidate R2 review passes with0 findings.
Release6b6cc71 passes actual GB10 text/4096-dimensional embedding/reranking
compatibility and its own1702.594s real-duration run:1605.030s success and
1700.001s provider timeout, one physical send each. Original workflow start,
configured budget and durable deadline match. R1's clock-renewal defect and its
causal regressions remain archived alongside historical release0596d114 results.
The [acceptance report](../providers-acceptance.md) links148 hashed original
receipts/logs/reports totaling228200B. Final candidate `6c23e1a` passed its 63.238s canonical gate and independent
R3 review with 0 findings. CI40560732248 passed; PR #37 merged as `a09d9d6`,
and #9 closed completed. The final gate/review originals are published in
[PR #37](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/37#issuecomment-6450384881).
Production daemon/provider and later CLI Runner acceptance stay distinct.

## Completed daemon: MVP-008

Candidate `7552d97` passed `bash scripts/check.sh` in78.23s:88 Rust tests,
8 Python tests, formatting, Clippy, release and capture acceptance. The installed
release reran all9 daemon cases, including SIGTERM/SIGINT, pending socket restart,
SIGKILL recovery and CLI/API parity. Kernel UID165536 controls and no-owned-TCP
checks passed. Independent R2 review passed with0 findings; CI40537161260 passed.
PR #36 merged as `a3d278d` with the same tree, and #8 closed completed. The four
R1 lifecycle repairs and the harness procfs-exit race retain causal RED/GREEN
receipts. [The protocol](../daemon.md) records the actual trust/coverage limits.

The user-requested dua audit found55.39GiB in the home checkout. Removing10.95GiB
of obsolete owned outputs and checksum-migrating retained artifacts leaves about
383MiB locally. The 2026-10-28 UTC dua recheck reports41.49GiB in .spike-work and13.65GiB in target
on the SSD mirror; the shared cache includes #10's first Lance/Arrow compilation.
Its temporary source checkout is200MiB and shares this target. All42 compressed corpus shards match the original
manifest; preserve them for #24 without retaining the84GB expanded copy. Both
large artifact roots remain lexical symlinks into `/ssd/mirror-rootfs/home/obj/`.
The existing full SSD database was not opened or modified.

## Completed capture: MVP-007

Candidate `41a984d` passed the complete canonical gate in 23.181 seconds and
independent whole-candidate R3 source review (0 findings). CI40506461591 passed;
PR #35 merged as `960320a`, and #7 closed completed. Nine capture fixtures,
four real capture SIGKILL points and the executable reconcile acceptance passed.
[The capture protocol](../capture.md) records exact originals, logical finals,
checkpoints, branch cutoffs and explicit gaps. R1's accepted-frame anchor race
and the saved-cursor sibling have ordinary causal RED/GREEN regressions. The
interrupted R2 has no final verdict and is not counted as approval. Production
IPC and real host parsers remain #8 and #12–#15 respectively.

## Completed history and receipts: MVP-006

Candidate `e98d4cb` passed 17 Domain and 17 Storage fixtures, the complete canonical
gate and independent whole-candidate R1 review (0 findings). CI40454932598 passed;
PR #34 merged as `42035bd`, and #6 closed completed. [The history contract](../history.md)
records native as-of reads, atomic scope membership, bounded fixed-point pages,
live read grants and exact RecallReceipt bytes. Native lookup can traverse a key's
version history; no constant-time hot-key or full-scale product claim is made.

## Completed provenance and adoption: MVP-005

Candidate `664e49c` passed 10 native domain fixtures, the complete canonical gate
and independent whole-candidate R1 review (0 findings). CI40450661442 passed;
PR #33 merged as `9b9f483`, and #5 closed completed. [The domain contract](../domain.md)
records source ceilings, trusted ingress boundaries, exact review grants, local
exceptions, latest/adopted separation and durable retry/reopen. These component
results do not authenticate actual host channels or implement the later review UI.

## Completed authority storage: MVP-004

Candidate `5654abf` passed the complete canonical gate, 16 Storage integrations,
six real SIGKILL points and independent whole-candidate R2 review (0 findings).
R1's cancelled-commit/checkpoint race is retained with its deterministic RED and
unchanged independent GREEN probe. CI40446328008 passed; PR #32 merged as
`95ff8f5`, and #4 closed completed. The [storage contract](../storage.md) and
[validation record](../storage-validation.md) preserve format, backup, migration
and tested-scope limits. These are native library results, not daemon acceptance.

## Completed storage selection: MVP-003

The tested Helix candidate failed three full-data low-memory recovery runs.
The selected sole backend is SurrealKV1.0.0 authority with LanceDB0.40.0/Lance13
reconstructible indexes. The [ADR](../benchmarks/storage-decision.md) records its
single compressed-authority content layout, measured settings, elimination
reason and limits. No production Storage module is claimed by the spike.

Source3b3994a/native860c32fa has passed its exact-HEAD root gate, independent R3
source review and Linux CI40088656119. Original-model binding passes139 commands
and67 identical before/after witnesses, with original deadlines and no reimport.
The final fixed3,840-second/100-client full concurrent measurement passes:

- 11,428,831 texts and1,000,000 real1536-dimensional vectors;50,000,002,783 unique
  UTF-8 source bytes. Reuse42 compressed shards; do not retain the84GB expansion.
- 108,641 completed requests,82,587 distinct seed/workload commits,10,391 expected
  write conflicts and zero operation errors. Both search types overlap the build;
  all workload latency, throughput, content and scope checks pass.
- RSS P99/max509,325,312/553,222,144B, no OOM. Build2,632.432s; total3,852.107s.
- 371 disk samples, total allocated peak137,661,698,048B and minimum free
  397,265,436,672B. All category caps pass. These are sampled allocation bounds.
- Completed reuse, exact process/unit exit and scratch cleanup pass. At measurement end, active3
  watermarks equal committed392298 and candidate5 is built. Build-end370032 is not
  the final workload commit.

The previous user-requested restart pause occurred after measurement completion.
No follow-up was launched during that pause, no new commit was made and no handoff
file was written. The six prepared follow-up phases now pass with514 native
commands:323 acknowledgement batches verify82,587 receipts; before/after witnesses
pass78/79 commands;26 vector-replay, three catch-up and five activation commands
pass. Catch-up takes552.448s; active5 and both watermarks equal392298. All exact
process/unit/scratch checks pass. The nohup pipeline terminates normally and its
WD relay is cancelled after collection.

[The verified archive](../benchmarks/results/fallback-full-rebuild-r3/summary.json)
contains2,116 files/22,486,670B, SHA256
`ae38a323030c3b5058550945a7b1368f270ae975147f75457b0337c18272539a`.
The final evidence candidate `8baaccc` passed the exact-HEAD root gate and
independent whole-candidate review (zero actionable findings). Linux CI
[40437740462](https://github.com/RyderFreeman4Logos/akasha-otsuge/actions/runs/40437740462)
passed; [PR #31](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/31) merged
as `13065fc`, and #3 closed completed. Full measurement was reused without rerunning.

The [matrix](../benchmarks/storage-evidence-matrix.md) connects every storage
criterion to evidence. Historical low/high1,800-second cases, burst/overload,
full serial lifecycle, direct-low interrupted recovery, reclamation and native
crash/corruption/ENOSPC fixtures retain their measured executable identities.
Both full concurrent OOM failures and unlogged unknown outcomes remain retained.
Product daemon, external workers, long model calls and four-host acceptance are
separate dependent requirements; no installed binary or user service was changed.

## Execution discipline

Load WD after every restart/compaction; use exactly one dynamic provider-TTL relay
while native work is live. Long executable tasks use `nohup`, detached stdin and
exact PID/start-time plus durable admission/progress/terminal receipts. Preserve
the original campaign start; intentional restart pauses are elapsed campaign time,
not active computation. Reconcile an exact terminal result before any replay.

After the current acceptance leaf is archived, run the canonical
`bash scripts/check.sh` on the exact committed candidate, obtain independent
review, push and read back the matching Linux CI, then merge and close the corresponding Issue.
Continue #12–#28 through publication in dependency order. Isolated preparation can
overlap immutable long acceptance while preserving one build writer. Avoid repeated full-corpus
runs on unrelated intermediate changes; #24 repeats the same manifest/oracle
through the actual product protocol. All required product work remains on the
complete TODO list; no scaffold or diagnostic result counts as its completion.
