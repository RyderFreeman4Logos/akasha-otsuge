VERDICT: FAIL

The frozen candidate has one MODERATE introduced defect: outcome records discard known log-source coverage information. The canonical gate and independent three-test check pass, but neither establishes the required incomplete-capture behavior.

**Candidate and custody**

| Identity | Verified value |
|---|---|
| Repository | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp021` |
| Branch | `feat/mvp-021-outcomes` |
| Base, origin/main and merge-base | `dc2134f92f0a99ded36df71d8284bf9046dd282d` |
| HEAD | `ee479760936864d2db7fd417e07fb6ad1343ead0` |
| Tree | `a2aa340f2341c7b3e6330496b5a2d8bb0a34e02e` |
| Range | `dc2134f92f0a99ded36df71d8284bf9046dd282d...ee479760936864d2db7fd417e07fb6ad1343ead0` |

Porcelain status was empty before review and at postflight. All 104 changed working-tree files were byte-compared with their frozen HEAD blobs and matched; the comparison was repeated at postflight. `git diff --check` passed.

No source edits, commits, worktrees, publication, agents, broad gate reruns or provider/native acceptance were performed. The invoking CLI owns report creation and hashing.

**Filesystem instructions and inspected scope**

Before inspecting source, I checked actual filesystem `AGENTS.md` and `CLAUDE.md` candidates at `/`, every ancestor of the repository, and the repository root. I also traversed nested source directories with symlink following. No instruction files were found. The nested traversal excluded `.git` and `target`; Git-tree absence was not used as evidence.

The repository’s logical and resolved paths are identical. Consequently, each relative source path below resolves under the repository root shown above.

| Changed production/test path | Lines |
|---|---:|
| `src/outcome.rs` | 313 |
| `src/daemon/outcome.rs` | 399 |
| `src/outcome_command.rs` | 273 |
| `src/outcome_command/workspace.rs` | 216 |
| `src/main.rs` | 882 |
| `src/ipc.rs` | 466 |
| `src/daemon.rs` | 769 |
| `src/daemon/jobs.rs` | 1,552 |
| `src/capture/mod.rs` | 559 |
| `src/domain/mod.rs` | 790 |
| `src/lib.rs` | 14 |
| `tests/daemon.rs` | 1,121 |
| `tests/support/daemon_outcome.rs` | 692 |

The complete changed source range was inspected with relevant shared context in:

- `src/adapter/binding.rs` — 290 lines; `src/adapter/context.rs` — 417.
- `src/capture/read.rs` — 234; `src/capture/schema.rs` — 181; `src/capture/file.rs` — 559.
- `src/domain/history.rs` — 629; `src/domain/recall.rs` — 565.
- `src/daemon/consult.rs` — 422.
- `scripts/check.sh` — 17; `scripts/check_daemon_command.py` — 197.

Required documents were read: `docs/spec/issues/MVP-021.md` (36 lines), `docs/outcomes.md` (88), and `docs/benchmarks/outcome-acceptance.md` (57). README changes, planning changes, archive classification, manifest and sample evidence were also inspected.

Review instructions used:

| Logical instruction path | Resolved path | Lines |
|---|---|---:|
| `/home/obj/.codex/skills/native-code-review/SKILL.md` | `/home/obj/.hermes/skills/software-development/native-code-review/SKILL.md` | 87 |
| `/home/obj/.codex/plugins/cache/ponytail/ponytail/5.3.3/skills/ponytail/SKILL.md` | Same logical path | 52 |

Applicable native-review references concerning attribution, exact-candidate evidence, Rust checks, receipt integrity and finalization were consulted.

**Finding — MODERATE, REGRESSION: incomplete source coverage disappears from outcome logs**

Location: `src/daemon/outcome.rs:194`, with admission at `src/daemon/outcome.rs:116`, the output schema at `src/outcome.rs:204`, and paged output at `src/daemon/outcome.rs:394`.

`outcome_log_events` accepts an authorized Capture event without inspecting its `coverage`. Submission then copies its retained raw bytes, hash and preview into `outcome::Log`, but does not preserve coverage or an incompleteness indicator. The paged log operation likewise returns retained bytes and `total_bytes` without that information.

A causal counterexample follows directly through the existing public paths:

1. Reconcile an authorized interchange Capture source with `host: null`, `coverage: "metadata_only"` and an empty raw payload. `CaptureInput::binding` permits this coverage for interchange sources, and Capture persists it in `EventRecord.coverage`.
2. Submit an otherwise valid outcome referencing that event within the invocation’s authorized scope.
3. Outcome admission checks reference existence and permissions, then accepts it.
4. The resulting log has zero bytes, an empty preview and `preview_truncated: false`. Paged reading returns zero `total_bytes` and no continuation. Neither response exposes the known metadata-only coverage.

An actually empty captured log and an unavailable payload therefore lose their essential distinction in the outcome interface. Hash verification still correctly authenticates the retained bytes; it does not establish source completeness.

This violates the explicit contract in `docs/outcomes.md:50`: incomplete source capture must remain visible. The structured-submission schema also provides no dedicated way to report a retained prefix as incomplete.

This is introduced by the new outcome projection, not a pre-existing Capture defect: Capture already preserves the coverage value, and the existing context reader checks reduced coverage before returning text.

Required repair: preserve source coverage and an explicit completeness/unknown state in immutable outcome logs and paged responses, or reject insufficient sources explicitly. Add a focused regression using a reduced-coverage Capture source. This finding is based on source tracing; no additional runtime probe was executed.

**Canonical gate evidence**

The existing exact-candidate `just check` was reused, not rerun.

Receipt:

`/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git/akasha-evidence/mvp021-canonical-r1-verified.json`

Receipt SHA-256:

`0a3d9873398dc4fc02135f0eed49a768f7cf48c41f539328a77255a1a8081f39`

All referenced artifact hashes were independently recomputed and matched, then reverified at postflight:

| Artifact | SHA-256 |
|---|---|
| Admission | `853dec6932ec0e7aa726334951505d96f686f348d6aa0c865a53e6cca417609e` |
| Terminal | `67b2cf6582668f1648ab79185ba1daa1fbc54d8ffab39453d8bb08a3dc27fa68` |
| Collection | `bb0b3e70243ee96a79dfa4443418f205fa09342f3ed55cf42fd596d23132346f` |
| Complete gate log | `b5e0f3c4a95bc932aacfb2e32431364b6679cc7dbdbb39e073fab4771f0f6a57` |

The gate exited 0 after 303.237 seconds. Its 20 Rust result rows total 461 passes, zero failures and 29 explicit ignores, including installed repeats. The daemon suite passed 123 tests with six opt-in ignores in each vector.

The log records compilation from this candidate checkout, release compilation, installed lifecycle checks, private-path and peer-UID denial, and zero TCP sockets. The installation checker compares installed and release executable hashes. The retained release executable independently hashes to the recorded installed value:

`4b6c57ee38f6d2e96bdc60ee5f0ded0995c4e74afaa5ae621ea2b50e2cde2180`

Thus the supplied evidence binds these installed checks to the rebuilt candidate executable, rather than merely assuming the earlier #20 binary was replaced.

**Independent focused check**

Executed exactly once:

```text
cargo test --test daemon outcome_tests:: -- --nocapture
```

Result: **3 passed, 0 failed, 0 ignored, 126 filtered out; exit 0**.

All three named integrations ran:

- `outcomes_bind_only_attached_receipt_revisions_and_survive_restart`
- `outcome_wrapper_records_dirty_builds_and_retains_full_logs_without_claiming_causality`
- `outcome_wrapper_retains_failures_with_missing_artifacts_and_binary_log_parts`

Complete combined stdout/stderr, exit code and output digest were preserved in:

`/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git/akasha-evidence/mvp021-whole-review-r1-tests.log`

Final log SHA-256:

`44edfa33dcec46520a085433756c778ae285cd49658da1451e1f3bc08cdf7a58`

Captured stdout/stderr SHA-256:

`fbed6cbd48c0d14a7d954b00793aad4f69b508bda032c9b3b2014a2b9c97cd75`

The inherited Rust 1.99.1, TMPDIR, target directory, sccache wrapper and sccache UDS were retained. `AKASHA_OUTCOME_EVIDENCE` remained unset. The Cargo shim emitted three read-only mise-cache warnings; compilation/test execution completed successfully. There was no retry.

**Archive verification**

The acceptance manifest SHA-256 matches the documented value:

`666ba3855c350232fbea07cc3978067aa33193cbd12e3472c30b61e41d7d9e54`

All **86 entries, totaling 103,858 stored bytes**, matched their lengths and hashes. Compressed entries also matched their uncompressed lengths and hashes. Every archived terminal log hash was verified.

Both build samples were checked against their records and independent oracle files:

- Each retained stdout contains 240,000 bytes, SHA-256 `25f83a9d05bad98e52dbf5f2ee8cd4dd5a08732ebcb91b4f80f04751f15a9d03`.
- Each compiler stderr contains 289 bytes, SHA-256 `d291e2b7f8d94ed4e6f4b5f7ba5e8bb4dc757f4c5096c567452c5350d21b1748`.
- Dirty-diff SHA-256 is `6d9df607dc6f57ea9d7e48b9715f6762292be269e3e7f95413219965e048df2f`.
- Recorded bases match their respective oracles. Repeated builds have distinct dirty-tree and artifact digests.
- The final historical three-test log matches `cf43fa9de326b1a5a4fded1161c4715b095d2afcd522fd2d36d5fcadbc3823bb`.

The three behavioral REDs remain distinguishable from compilation failures and incorrect fixture assumptions. The first archived build case does not erase its enclosing suite failure; the second belongs to the complete passing suite.

**Other audited boundaries**

No additional actionable introduced defect was established in the reviewed range.

Caller identity and scope derive from the verified stored invocation. Receipt attachment checks the actual successful Consult job, binding and saved RecallReceipt; claims require exact selected revisions. Delivered relations derive from Domain acknowledgement, while caller claims cannot manufacture `attached` or `returned`. Stored attribution remains immutable when later delivery changes.

Current grants are checked on submission, replay, reads, log capture and log reads. Source spans retain hash validation and scope checks. The owner-scoped evidence fixture legitimately reads another project source without attaching its unselected memory.

The wrapper runs the explicit argv and records exit, signal or spawn error. Missing Git produces coverage gaps. Dirty observations include tracked and nonignored untracked files. Relative artifact validation and descriptor-relative, no-symlink regular-file reads prevent artifact traversal; missing/unsafe artifacts become gaps without discarding the command failure.

Log transport is bounded, paged reads are lossless, and preview hashes refer to retained raw sources. Capture verifies immutable raw-part hashes. Exclusive event markers prevent local command replay; failure paths retain the marker and available diagnostic spool, while successful publication retains a receipt. These mechanisms do not promise an atomic filesystem snapshot or distributed exactly-once execution.

Structured submissions remain attributed reports, not independently executed attestations. Nonzero exits and environment failures do not automatically create incidents or causal conclusions.

Skipped as requested: broad gate reruns, provider/native authentication acceptance, publication and incident investigation.
Material risk: incomplete Capture sources currently lose their coverage warning when projected into outcomes; the passing gates do not cover that case.

VERDICT: FAIL