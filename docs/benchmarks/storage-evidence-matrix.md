# Storage acceptance evidence — MVP-003

Status: **incomplete; no backend selected**. The current candidate includes shared-session78ceb79 plus the derived-admission repair,
SurrealKV1.0.0 plus pinned LanceDB0.40.0/Lance13. The authoritative
[contract](storage-contract.md) and [resource specification](../spec/08.md)
retain their original thresholds. This table separates historical measurements,
current-source regressions and remaining full-scale work; passing a small
fixture does not establish a full-scale resource result.

| Required behavior | Evidence | Proven scope and remaining boundary |
| --- | --- | --- |
| Actual50GB content and million-vector corpus | [Full import/history](results/fallback-full-import-history/summary.json) | All11,428,831 originals,50,000,002,783 unique UTF-8 bytes and1,000,000 real1536-dimensional vectors verified at low budget.42 compressed shards reused; expanded84GB stream not retained. |
| Atomic revision/head/receipt/outbox and idempotence | [Authority](results/fallback-authority/summary.json), [acknowledgement verification](results/fallback-full-rebuild-recovery/summary.json) | Small native crash/negative controls plus9,336 full-run recorded acknowledgements verified against durable content and request hashes. Ten unlogged outcomes from the failed run remain unknown. |
| Deterministic100k history, scope exceptions and fixed read points | [Verification identity](results/fallback-verification-identity/summary.json) |5,163 checks at the frozen historical point. Later commits do not permit replaying that verifier with an unchanged expectation. |
| Full lexical/vector coverage and scoped candidates | [Decision record](storage-decision.md) | Complete native coverage,64 scoped queries and1,195 authority-checked candidates.32 ANN queries use exact references over the million vectors; mean recall@20 is0.9125. Candidate-set merging does not promise stable/global ranking. |
|512MiB and2GiB steady workloads | [Low](results/fallback-full-sustained-low-r3/summary.json), [high](results/fallback-full-high-burst-overload-r3/summary.json) | Both historical1800s/100-client cold mixed cases pass. The summaries retain their measured native identities. Current shared-session full concurrent acceptance is pending; old metrics are not relabeled as current measurements. |
| Burst, overload and bounded drain | [Burst/overload](results/fallback-full-high-burst-overload-r3/summary.json) | Low-profile write-only burst100/100; overload6,507 successes/793 expected conflicts, no operation errors,1.434s drain. These are not mixed-search or cold cases. |
| Large backlog and direct high-to-low reopen | [Profile drain](results/fallback-full-profile-drain/summary.json) | Full100k backlog and37.658s controlled transition preserve original deadlines. Production daemon reload integration remains#8/#24. |
| Current-authority filtering, real corrections and permission changes | [Full corpus lifecycle](results/fallback-full-corpus-lifecycle/summary.json) |16 candidates,64 revisions and755 native commands, including real BM25/ANN originals. Serial full-data proof; the concurrent requirement remains separate. |
| Interrupted reindex recovery and activation barriers | [Full OOM recovery](results/fallback-full-rebuild-recovery/summary.json), [current lifecycle](results/fallback-shared-session/summary.json) | Same-native direct-low full recovery passes within its original deadline. Current179-command fixture covers12 real SIGKILL barriers and two competing owners. No hardware power-loss claim. |
| Authority corruption and disk exhaustion | [Import recovery](results/fallback-import-recovery/summary.json) | Owned authority/WAL/VLog corruption and actual kernel ENOSPC fixtures. Explicit errors and retained committed data; no host disk filling. |
| Derived corruption and reconstruction | [Derived corruption](results/fallback-derived-corruption/summary.json) |85 checks across both native index families. Offline quarantine/reconstruction; automatic daemon recovery remains dependent work. |
| Invalid config, snapshot pinning and competing reloads | [Configuration tests](../../tests/config.rs) |17 product ConfigManager tests. Production storage consumers and worker lifecycle remain#8/#9/#24; model1700s acceptance remains#9. |
| Safe physical space reclamation | [Pruning](results/fallback-full-prune/summary.json), [retirement](results/fallback-full-retirement/summary.json), [generation4 preparation](results/fallback-full-generation4-preparation/summary.json) | Full public native cleanup preserves authority/history/receipts. Current preparation verifies66 before/after witnesses and removes disposable binary copies after reuse checks. Inode allocation and filesystem free space are separate measurements. |
| One shared native cache budget per authority | [Shared-session repair](results/fallback-shared-session/summary.json) | Actual native identity assertion fails before the repair, passes after it, and checks fresh rows/indexes.12 release units,179 lifecycle commands and4,528 diagnostic concurrent requests pass. This is not full resource acceptance. |
| One admitted baseline/delta index builder | [Derived admission](results/fallback-derived-admission/summary.json) | Real held-delta regression proves the old bypass and repaired expiry/write progress.12 units,179 lifecycle commands and a4,528-request concurrent diagnostic pass. Initial7.904s write-P95 failure is retained; one later baseline/candidate pair passes without changing thresholds. Full resource acceptance remains open. |
| Full build overlapping ordinary writes, both searches and corpus corrections | [Full r2 failure](results/fallback-full-rebuild-r2/summary.json) | Original run failed OOM at403.420s; shared-session r2 also fails OOM at1762.242s, RSS P99/max579,792,896/608,862,208B and16 recorded search errors. No successful concurrent acceptance. |
| Generation-specific receipts/watermarks through a successful concurrent rebuild | [Full r2 reconciliation](results/fallback-full-rebuild-r2/summary.json) |39,876 complete acknowledgements verified; eight originals restored and active3 watermarks reach309711. One unlogged commit and one incomplete log line remain unknown. Generation4 remains partial/inactive; successful rebuild/catch-up/activation still required. |
| Complete physical rebuild bound | [Full r2 disk samples](results/fallback-full-rebuild-r2/summary.json) |159 samples remain within category caps, total observed peak123,707,609,088B. Both failed runs lack a completed rebuild peak. Ten-second inode-allocation samples do not prove an instantaneous quota or exclusive Btrfs extents. |

The specified interruption coverage is the real full OOM/recovery plus named
transaction/index/activation SIGKILL barriers. The specification does not require
an additional duplicate full-build kill after these checks. No accepted criterion
is removed by that scope decision.

MVP-003 finishes only after a successful full case and its follow-up evidence,
review, sole-backend ADR and PR merge. The production Storage, daemon, host
integrations and final protocol/resource gates remain#4–#28.
