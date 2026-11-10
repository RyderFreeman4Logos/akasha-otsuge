VERDICT: FAIL

The complete seven-commit range contains one LOW introduced defect in the reference plugin’s invalid-input preservation boundary. All four requested focused commands passed, and the supplied canonical and archive evidence validates. Under the requested rule that actionable introduced defects fail the range, the overall verdict is FAIL.

**Frozen identity and custody**

| Identity | Verified value |
|---|---|
| Repository | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp023` |
| Branch | `feat/mvp-023-notifier` |
| Base, origin/main and merge-base | `6eb77a1345a5f46f245497ceae1e77f70b68b2bb` |
| HEAD | `24ca2818d4ada599f6d91409cf38d85a84acd47d` |
| Tree | `8305878ab05b60234f80c44573afb609f05b972b` |
| Scope | Complete `origin/main...HEAD`, seven commits |
| Initial/final status | Clean porcelain output |
| Changed-file verification | All 148 changed files equal their frozen HEAD blobs |
| Diff whitespace check | Passed |

Reviewed commits, oldest first: `9b97da0`, `2afb8da`, `64170ed`, `91cea99`, `cde76ab`, `03acdcc`, `24ca281`.

The full-index binary range diff contains 482,726 bytes and hashes to:

`5a28121e7d21f50292a1c6cb801e701029b9217f69e807689d3ab75177c55c41`

Source remained read-only. No edits, commits, publication, worktrees, agents, canonical rerun, provider acceptance, configuration changes or environment overrides occurred. WD remained parent-owned. The final checkout-scoped process census found no Cargo, rustc, Just or daemon process.

This is the complete report for the parent to preserve at the requested report path and hash.

**Instructions and source coverage**

Before source inspection, filesystem checks examined AGENTS.md and CLAUDE.md in the checkout, logical and resolved ancestor chains through `/`, corresponding `/home/obj/project/github/RyderFreeman4Logos` ancestors, and nested source directories using symlink-following traversal. No applicable instruction files were found: **zero paths, zero readable lines**. This conclusion came from filesystem inspection, not Git absence.

Instruction sources applied:

- `/home/obj/.codex/skills/native-code-review/SKILL.md`, with applicable scope, evidence, lifecycle, attribution, verdict, Rust-isolation and finalization references.
- `/home/obj/.codex/plugins/cache/ponytail/ponytail/5.3.3/skills/ponytail/SKILL.md`.
- `.review/mvp023-stdio-r3-brief.md` — 33 lines.

Requirements inspected:

| Path | Coverage |
|---|---|
| `docs/spec/issues/MVP-023.md` | All 36 lines |
| `docs/spec/07.md` | Notifier contract, sections 5–6 and adjacent acceptance |
| `docs/spec/04.md` | Notification exception and live controls |
| `docs/notifications.md` | All 150 lines |
| `docs/notifier-protocol.md` | All 123 lines |
| `docs/plan/todo.md` | Operating rules and active MVP-023 checklist |
| R1/R2 archived reports | Both immutable FAIL reports and their findings |

The complete changed production surface, affected tests, fixtures and documentation were reviewed with shared context in:

- `src/daemon/notifier.rs`, `src/daemon/notifier/stdio.rs`, `src/daemon.rs`, `src/daemon/jobs.rs`.
- `src/adapter/mod.rs`, `src/adapter/guardian.rs`, `src/adapter/native.rs`.
- `src/config/schema.rs`, live route selection in `src/config/manager.rs`, shared IPC errors and identifier validation.
- Storage record limits and bounded key pagination.
- `examples/notifier-file.py`, `scripts/test_notifier_file.py`.
- `tests/config.rs`, `tests/support/daemon_notifier.rs`, `tests/fixtures/notifier/oneshot.py`.
- README changes and all three evidence archives.

PR #54’s merged local-inbox implementation was treated as the base.

**Finding**

**1. LOW — REGRESSION: an invalid update consumes durable `--fail-first` state before its target is validated.**

Location: [examples/notifier-file.py:118](/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp023/examples/notifier-file.py:118), preceding update-target validation at line 124.

After loading valid state and checking receipt replay, `deliver()` persists `failed_once=True` before checking whether a new update identifies an existing provider message.

A bounded probe invoked the actual candidate executable once against an isolated private fixture:

1. The initial valid state contained empty `messages` and `receipts`, with `failed_once=False`.
2. The request had a valid envelope shape, `action="update"` and the correctly derived provider ID for `notice-1`.
3. That provider message did not exist.
4. The executable ran with `--fail-first`.

Observed result:

| Observation | Value |
|---|---|
| Process exit | `75` |
| Stdout | Empty |
| Stderr | `NOTIFIER_FILE_FAILED` |
| Messages after refusal | Empty |
| Receipts after refusal | Empty |
| `failed_once` after refusal | `true` |
| State bytes preserved | **No** |

Before-state SHA-256:

`c8b7e8e2aef7c36476fe991022d7ab4f94fe3dfdc9175f58cf2f1f6038843ac0`

After-state SHA-256:

`079572c983a1e6a7816d08f9002715fba912589622dc1c82fdefe02bf0275609`

Thus rejected semantic input changes valid durable state and consumes the demonstration failure. The effect is limited to that marker; the probe created no message or receipt and returned no false success.

**Required correction:** validate a new update’s target under the lock before persisting the demonstration failure. Preserve the existing receipt-replay path. Add a small regression asserting unchanged state bytes for this invalid update with `--fail-first`.

**Attribution:** REGRESSION. The executable and this ordering are introduced entirely within the reviewed range; the base has no reference file plugin.

This is LOW severity. It remains actionable under the review’s explicit invalid-input preservation requirement, and the supplied verdict rule makes it range-blocking.

**Recovery and ownership review**

The previous R1/R2 counterexamples are repaired. The remaining source review found no additional actionable defect:

| Boundary | Review result |
|---|---|
| Native execution errors | `Delivery::fail()` normalizes reused native unavailable, failure and timeout codes into the finite notifier policy. Legacy terminal native codes enter that policy once. |
| Terminal/control distinctions | Output/protocol/configuration failures retain terminal handling. Read denial, disable, cancellation and route change block without consuming or resetting transient failures. |
| Attempts and backoff | Separate claim/failure counters persist. Automatic retries use 1/2/4/8-second backoff and stop after five transient failures. |
| Destination changes | Channel/destination hashes select retained state. Failed A permits B; A/B/A retains A’s exhausted budget or pending accepted request. Foreign provider IDs are not reused. |
| Source advancement | Pending retained requests replay their accepted envelope/key before a newer generation. Result publication reloads the current notification and preserves live source state. |
| Successful generations | Current successful generations remain suppressed across route changes, reload and restart. A genuinely newer generation obtains its own key/budget after the preceding request settles. |
| Active deadlines | One monotonic deadline spans capabilities and delivery. Policy-only reloads preserve the active timeout and destination identity. |
| Permissions and cancellation | Live read grants and enabled routes are checked before execution and during exchange. Route/credential-reference changes cancel owned execution. |
| Duplicate risk | Interrupted non-idempotent sends expose uncertainty. Capability declarations remain provider assertions, not universal exactly-once proof. |
| Process ownership | Guardian cleanup covers descendants, including escaped sessions. Detached reaping retains the semaphore permit and scratch cleanup through cleanup continuation. |
| Bounds | Requests are bounded at 8 KiB; each output stream at 4 KiB; provider IDs at 256 bytes. Histories and record admission are bounded without eviction-based budget renewal. |
| Paging | Record and byte limits coexist; cursors advance across filtered scopes and bounded batches. |
| Redaction | Unrecognized plugin output, stderr, command text and credential values are not copied into durable delivery errors. |
| File effect/receipt | Locking and fsynced atomic replacement keep successful effects and receipts together. Nested-state validation and valid old-key replay repairs remain intact. Finding 1 concerns validation ordering before a demonstration failure. |

The grouped-incident fixture submits 100 reports through the real reference executable, observes its durable first failure and retry, verifies escalation and three generation receipts, and asserts that no model Runner starts.

**Fresh focused verification**

Each requested command ran once, sequentially, without retries:

| Command | Passed | Exit |
|---|---:|---:|
| `python3 scripts/test_notifier_file.py` | 4 | 0 |
| `cargo test --locked --test config notifier -- --nocapture` | 2 | 0 |
| `cargo test --locked --lib notifier -- --nocapture` | 4 | 0 |
| `cargo test --locked --test daemon notifier_tests:: -- --nocapture` | 22 | 0 |

Daemon runtime: **40.55 seconds**.

Complete output and four numeric exit markers are preserved at:

`/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git/akasha-evidence/mvp023-stdio-review-r3-tests.log`

- Bytes: **5,863**
- SHA-256: `e6751670df83af107acb12ed47874d2ba0cf488eac47c008b4fb05647264412a`

Inherited Rust `1.99.1`, TMPDIR, shared Cargo target and sccache were retained. Cargo resolved through the mise shim and reported `cargo 1.99.1`. Denied mise cache-write warnings appeared; all requested commands exited zero.

The finding’s additional isolated probe was separate from these four commands.

**Canonical evidence**

The existing R3 verified receipt and every referenced artifact were independently hashed. Admission/terminal identities match the frozen candidate; admission records an empty dirty diff and no untracked files.

| Artifact | SHA-256 |
|---|---|
| `mvp023-stdio-canonical-r3-verified.json` | `35d9fa313295858314bb8d68b2511600c30437e8fe514b992c75df5a839fb50b` |
| Admission | `fb17d15ab0e61bac32ae1fc7597c28fe6bca35f40312e3adeb4a99c617e240af` |
| Terminal | `542e9c3d828778df341e2f0991c4f561043e7716c3d4b35df6ca49c10a1f6b0a` |
| Collected receipt | `1e9a8372557e202221e33712639c2f400f498e60064b02cb0ec054afef0d4768` |
| Complete log | `31b863e2de8e1e0b851d6254b56c73bc95034bb28703e328bd6c5c49a683a604` |

The raw log reconciles with the receipt:

- `just check`: exit **0**, **418.027 seconds**.
- **20** Rust result rows; **535 passed**, **31 ignored**, including installed repeat.
- **155 daemon passes**, **7 ignored**, per daemon run.
- **33 Python tests passed**.
- Installed lifecycle, private-UID denial and peer-UID denial passed; zero TCP sockets reported.
- Installed binary SHA-256: `e56b383adc067cc8b800aca87bfa3340cbca489bc4e277a677046eeaa4d8be55`.

These are validated existing exact-candidate receipts. The canonical gate was not rerun.

**Archive validation**

| Archive | Manifest SHA-256 | Members | Bytes excluding manifest |
|---|---|---:|---:|
| `mvp023-stdio` | `cb84358dfd02682307ab8a23c898495a7ae13f64d8ff51ea00aa9980ee305a8d` | 77 | 58,680 |
| `mvp023-stdio-r1-fixes` | `0ebca0a3a0f6072bb6b28950ce0ee99d491d618d50dc308b46487f728bf7c9f2` | 19 | 74,794 |
| `mvp023-stdio-r2-fixes` | `049366157f5670190411dbcab0ab46bf23f53dc6bd208449018e7466eb70fe57` | 33 | 108,093 |

All member sets, byte counts and hashes match. All gzip members decompress. All **26 terminal/log pairs** match their uncompressed hashes; both archived CI logs also match their compressed and uncompressed receipt hashes.

Each archive’s 12 source hashes was checked against its proper Git identity:

- Original: `cde76abe7e8a45288d06919a93a7d06f940c10ee`.
- R1 repairs: `03acdccb102daf3d6556ed21d19c125b3648b8bb`.
- R2 repairs: current candidate `24ca2818d4ada599f6d91409cf38d85a84acd47d`.

Historical distinctions remain intact:

- Phase-1 stdin EOF repair; historical paging timeout remains unattributed.
- Retry/crash behavioral RED.
- Policy RED and corrected timeout fixture.
- Missing reference executable.
- Two byte-page compile setup failures before behavioral `RESULT_TOO_LARGE`.
- Record-capacity refusal RED.
- Original complete focused result: 18 daemon, 3 Python and Clippy.
- R1 causal RED and repaired focused result: 19 daemon, 4 Python, 3 units and Clippy.
- R2 causal RED: three executed failures.
- R2 first aggregate result: 10 passes and 12 IPC/socket-read failures.
- Unchanged-source isolation: three causal passes.
- Later observed complete result: 22 daemon passes plus Clippy; four Python, four unit and two config checks also passed.

The observer collects process metadata/PSI. Its passing observation does not establish the historical IPC failure’s cause. Pre-format admission snapshots remain distinct from the clean canonical proof.

Both immutable FAIL reports match their publication-receipt hashes:

- R1: `afddad0500701e701850d12fe63ce4e609507699ccfc44eb32b61ef2f97e68ca`.
- R2: `f4fbf55aff72ea0a639ca8fa3323d0b682638aa76cdc231d3bf903f0754bc7b7`.

The archived receipts record verified publication readback. No new publication or remote readback occurred during this review.

Older canonical and CI success remains bound to the older candidates: CI `42739540908` to `cde76abe…`, and CI `42742670727` to `03acdcc…`. Neither establishes current-head CI success.

Skipped: canonical rerun, broader gates, new CI/publication and provider acceptance, as requested. Risk: the LOW invalid-input mutation above blocks this range under the supplied verdict rule; #12 and #24–#28 remain separate acceptance work.

VERDICT: FAIL