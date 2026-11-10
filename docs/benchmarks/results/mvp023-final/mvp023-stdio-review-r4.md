VERDICT: PASS

The complete eight-commit stdio-notifier range has **zero actionable REGRESSION findings**. All four requested focused commands passed once. The supplied exact-candidate canonical evidence and all four archives validate. The R1, R2 and R3 defects are repaired.

This is the complete report for the parent to preserve unchanged at `/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git/akasha-evidence/mvp023-stdio-review-r4.md` and hash.

**Frozen identity and custody**

| Identity | Verified value |
|---|---|
| Repository | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp023` |
| Branch | `feat/mvp-023-notifier` |
| Base, origin/main and merge-base | `6eb77a1345a5f46f245497ceae1e77f70b68b2bb` |
| HEAD | `fc88baf240d710f71a91f5c886d9fec4e21929f2` |
| Tree | `4cd08d029733fe564284f5760bda91a4179f7a24` |
| Range | Complete `origin/main...HEAD`, eight commits |
| Initial and final status | Clean porcelain output |
| Changed-file custody | All 166 changed files equal their frozen HEAD blobs |
| Diff whitespace check | Exit 0 |

Commits reviewed, oldest first: `9b97da0`, `2afb8da`, `64170ed`, `91cea99`, `cde76ab`, `03acdcc`, `24ca281`, `fc88baf`.

The full-index binary range diff is **543,139 bytes**, SHA-256:

`902ae0c43ec68aec71f3f662b67f99ee61eb03f432e6757f266caf0e8c345a49`

Source remained read-only. No edits, commits, publication, worktrees, agents, canonical rerun, provider acceptance, model calls, configuration changes or environment overrides occurred. WD remained parent-owned. Final checkout-scoped process metadata showed no remaining Cargo, rustc, Just, daemon or Python process other than the inspecting process.

**Filesystem instructions and source coverage**

Before source inspection, actual filesystem checks examined `AGENTS.md` and `CLAUDE.md` in the checkout and its logical/resolved ancestors through `/`, including corresponding `/home/obj/project/github/RyderFreeman4Logos` ancestry. Nested checkout traversal followed symlinks.

No applicable files were found: **zero instruction paths, zero readable lines**. This conclusion uses filesystem inspection, not Git absence.

Other instruction sources read:

| Path | Lines |
|---|---:|
| `/home/obj/.codex/skills/native-code-review/SKILL.md` | 87 |
| `/home/obj/.codex/plugins/cache/ponytail/ponytail/5.3.3/skills/ponytail/SKILL.md` | 52 |
| `.review/mvp023-stdio-r4-brief.md` | 37 |

Applicable native-review references covered scope attribution, evidence, lifecycle, subprocess ownership, Rust isolation, receipt integrity and frozen-report custody.

Requirements inspected included:

- `docs/spec/issues/MVP-023.md` — all 36 lines.
- `docs/spec/07.md` — sections 5–6 and adjacent acceptance.
- `docs/spec/04.md` — notification exception and live-control semantics.
- `docs/notifications.md` — all 150 lines.
- `docs/notifier-protocol.md` — all 123 lines.
- `docs/plan/todo.md` — operating rules, active MVP-023 acceptance and separate issue boundaries.
- All three immutable prior full FAIL reports and repair READMEs.

The complete production change and affected tests/docs were reviewed with shared context in:

- `src/daemon/notifier.rs`, `src/daemon/notifier/stdio.rs`, scheduler ownership in `src/daemon/jobs.rs`, and service startup/shutdown in `src/daemon.rs`.
- `src/adapter/mod.rs`, `src/adapter/guardian.rs`, `src/adapter/native.rs`.
- `src/config/schema.rs`, live route selection in `src/config/manager.rs`, duration validation, shared IPC error conversion and identifier validation.
- Storage record limits and bounded key pagination.
- `examples/notifier-file.py`, `scripts/test_notifier_file.py`.
- `tests/config.rs`, `tests/support/daemon_notifier.rs`, `tests/fixtures/notifier/oneshot.py`.
- README, protocol, notification documentation, active TODO and all four evidence archives.

PR #54’s local-inbox implementation was treated as the merged base.

**Findings and repair reassessment**

Actionable REGRESSION findings: **0**. No PRE-EXISTING finding is asserted by this review.

The prior defects are closed:

- **R1 nested-state integrity:** the file plugin validates retained message structures, receipt structures, references and current-message hashes before replay or mutation. Malformed and dangling state fails without changing state bytes. Valid historical receipt replay remains available after message updates.
- **R1 fifth-claim cancellation:** claim count and transient-failure count remain separate. Policy cancellation blocks without consuming or renewing failures. The sixth claim can resume the accepted request; its next transient failure still exhausts a previously four-failure budget.
- **R2 native failures:** `Delivery::fail()` normalizes reused native unavailable, failure and timeout codes into the finite notifier retry policy. Legacy terminal native-error records enter that policy once.
- **R2 destination recovery:** retained channel/destination state permits failed A → new B while preserving A’s exhausted budget. A/B/A preserves pending accepted input despite source advancement.
- **R3 invalid-update preservation:** update-target validation now precedes `failed_once` persistence, under the existing lock and after accepted-receipt replay. Missing and mismatched targets preserve state bytes; valid replay still bypasses demonstration failure.

The R3 archive’s RED is behavioral: the real executable changed `failed_once` and serialized state bytes before rejecting a missing update target. Its GREEN exercises the repaired ordering. The fresh five-case Python run also passed.

**Execution, persistence and boundary review**

| Boundary | Review conclusion |
|---|---|
| Retry accounting | Transient failures persist 1/2/4/8-second backoff and stop after five failures. Policy controls neither consume nor reset that budget. Counters saturate rather than wrap. |
| Error classes | Protocol/output/configuration refusals remain terminal. Read denial, disable, cancellation and route change remain blocked controls. Native execution errors use the shared finite policy. |
| Interrupted recovery | Old-owner preparing/sending state enters recovery. Accepted envelopes, keys, original start and consumed failures survive restart. |
| Destination state | History is selected by channel plus destination hash. A different destination receives its own key and no foreign provider ID. Returning to an exhausted destination does not renew its same-generation budget. |
| Source advancement | Pending accepted older requests replay before a newer generation at that destination. Result publication reloads the current notification and preserves concurrent source updates. |
| Successful generations | A successful current generation is suppressed across reload, route changes and restart. A genuinely newer generation can obtain its own key after settlement. |
| Active timeout | One monotonic deadline spans capabilities and delivery. Policy-only reloads preserve destination identity and do not extend the active deadline. |
| Live controls | Dispatch checks current read permission and route availability before execution and during exchange. Command or credential-reference changes cancel the owned process. |
| Capabilities and uncertainty | Unsupported update falls back to create. Interrupted non-idempotent sends expose duplicate risk. Capability declarations remain provider assertions, not independent exactly-once proof. |
| Ownership and release | Dedicated notification admission, guardian descendant ownership and detached reaping retain the semaphore lease through cleanup continuation. Scheduler shutdown drains notification tasks; daemon death is observed by the guardian. |
| Protocol and redaction | Requests are bounded at 8 KiB; stdout/stderr individually at 4 KiB; returned IDs at 256 bytes. Errors do not persist arbitrary plugin output, stderr, command text or credential values. |
| History and record limits | Channel and displaced-destination histories are bounded separately. Refusal preserves retained state instead of evicting it to renew budgets. Claim admission reserves space below the 128 KiB storage limit. |
| Pagination | Record and serialized-byte bounds coexist. Cursors advance across scope-filtered records and byte-limited pages. |
| Config compatibility | Added defaults are omitted from serialization as required for existing digest/fingerprint compatibility. Live policy changes retain the established destination identity. |

The reference plugin’s successful effect and receipt share one process lock and fsynced atomic replacement. Concurrent same-key callers return one durable result. Input/state validation precedes mutation; writes exceeding the 2 MiB bound preserve the previous state. Historical receipts remain retained rather than silently expired. These guarantees match its documented bounded file-inbox role.

The grouped-incident test drives the actual reference executable through a durable first failure, retry, 100 grouped reports and priority upgrades. It verifies one retained provider message, three generation receipts, minimal content and absence of a model Runner start.

**Fresh focused verification**

Each requested command ran exactly once, sequentially, without retries:

| Command | Passed | Exit |
|---|---:|---:|
| `python3 scripts/test_notifier_file.py` | 5 | 0 |
| `cargo test --locked --test config notifier -- --nocapture` | 2 | 0 |
| `cargo test --locked --lib notifier -- --nocapture` | 4 | 0 |
| `cargo test --locked --test daemon notifier_tests:: -- --nocapture` | 22 | 0 |

Daemon runtime: **40.57 seconds**.

Complete output and four numeric exit markers are preserved at:

`/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git/akasha-evidence/mvp023-stdio-review-r4-tests.log`

- Bytes: **5,856**
- SHA-256: `78b61aa9009f6adc9d702c65ece0820d2d71764f522a556d3426d2497a189fc4`

Inherited settings remained intact: Rust `1.99.1`, supplied TMPDIR, shared Cargo target and sccache wrapper/socket. Cargo resolved through the mise shim. Denied mise cache-write warnings appeared; every requested test command exited zero.

No additional runtime probe was needed.

**Canonical evidence**

The supplied R4 receipt and all referenced artifacts were independently hashed. Admission identifies the exact HEAD/tree, an empty dirty diff and no untracked files. Terminal and collected receipts bind the successful command and final log.

| Artifact | SHA-256 |
|---|---|
| Verified receipt | `b7f533ca26d9e8be307a9f76ab059635b94ac090de835c18e986379b1d080e28` |
| Admission | `eb3df71f2e050c03044cc09e20225ecf5a3fee1e4a9454abbddb3410b775cd4f` |
| Terminal | `e510ae01442a86374f22dc5fd0148a2bca80a287894fdcd5f30cf4e809235454` |
| Collected | `26fd83e1f61b8d380f64a223bcc84d83a36a0b2991c7f3b483acf380a1fa8c51` |
| Complete log | `76ae86c364ef151696d2595010504abcdae5b1f0df1031dbbe03666f1d9e8e79` |

The existing `just check` evidence records:

- Exit **0**, **407.988 seconds**.
- **20** Rust result rows: **535 passed**, **31 ignored**, including installed repeat.
- **155 daemon passes**, **7 ignored**, per daemon run.
- **34 Python tests passed**, as shown by the raw R4 log.
- Installed lifecycle, private-UID denial and peer-UID denial passed; zero TCP sockets reported.
- Installed binary SHA-256: `e56b383adc067cc8b800aca87bfa3340cbca489bc4e277a677046eeaa4d8be55`.

This validates existing exact-candidate gate evidence; the canonical gate was not rerun.

**Archive validation**

| Archive | Manifest SHA-256 | Members | Bytes excluding manifest |
|---|---|---:|---:|
| `mvp023-stdio` | `cb84358dfd02682307ab8a23c898495a7ae13f64d8ff51ea00aa9980ee305a8d` | 77 | 58,680 |
| `mvp023-stdio-r1-fixes` | `0ebca0a3a0f6072bb6b28950ce0ee99d491d618d50dc308b46487f728bf7c9f2` | 19 | 74,794 |
| `mvp023-stdio-r2-fixes` | `049366157f5670190411dbcab0ab46bf23f53dc6bd208449018e7466eb70fe57` | 33 | 108,093 |
| `mvp023-stdio-r3-fixes` | `a5e1df192937c0532794a09869af6abb56436c2ea081edd57c24f515371e5ef3` | 17 | 42,207 |

All member sets, byte counts and hashes match. Every gzip member decompresses. All **29 terminal/log pairs** match their uncompressed hashes; both archived CI logs match compressed and uncompressed receipt hashes.

Each archive’s 12 source hashes was verified using `git show` at its proper binding:

- Original: `cde76abe7e8a45288d06919a93a7d06f940c10ee`.
- R1: `03acdccb102daf3d6556ed21d19c125b3648b8bb`.
- R2: `24ca2818d4ada599f6d91409cf38d85a84acd47d`.
- R3 repair: current candidate `fc88baf240d710f71a91f5c886d9fec4e21929f2`.

The archives preserve these distinctions:

- Phase-1 stdin EOF repair; historical paging timeout remains unattributed.
- Retry/crash behavioral RED.
- Policy RED and corrected timeout fixture.
- Missing reference executable.
- Two byte-page compile setup failures before behavioral `RESULT_TOO_LARGE`.
- Record-capacity refusal RED.
- Original complete focused result: 18 daemon, three Python and Clippy.
- R1 repaired focused result: 19 daemon, four Python, three units and Clippy.
- R2 causal RED: three executed failures.
- R2 first aggregate failure: 10 passes and 12 IPC/socket-read failures.
- Three unchanged-source isolated causal passes, followed by the complete observed 22-test PASS and Clippy; four Python, four unit and two config checks also passed.
- R3 invalid-update behavioral RED, then five Python cases and the real grouped-reference-plugin daemon case passing.

The R2 observer reads process metadata and PSI. Its later passing observation does not establish the historical IPC cause. Pre-format admission snapshots remain distinct from clean canonical candidate proof.

All three immutable FAIL reports match their publication-receipt hashes:

| Report | SHA-256 |
|---|---|
| R1 | `afddad0500701e701850d12fe63ce4e609507699ccfc44eb32b61ef2f97e68ca` |
| R2 | `f4fbf55aff72ea0a639ca8fa3323d0b682638aa76cdc231d3bf903f0754bc7b7` |
| R3 | `2336ab31e12ceccdb82a6321f91b09be3d15738dfbb8afe587002b1c84e17882` |

Their receipts record verified publication readback. No new remote readback or publication occurred here.

Older canonical and CI successes remain tied to their explicit candidates. CI `42739540908` binds `cde76abe…`; CI `42742670727` binds `03acdcc…`. The R3 archive makes no prior-head CI-success claim. None establishes current-head hosted CI success.

The reviewed functional stdio acceptance is supported by source and focused evidence. Current-head CI, publication, merge and issue closure remain parent-owned stages. Authentication #12, stress #24, migration #25, packaging #26, model quality #27 and final release #28 remain separate.

Skipped: canonical rerun, broader gates, new CI/publication and provider acceptance, as requested. Risk: provider idempotency remains declarative; historical IPC failure causality remains unknown.

VERDICT: PASS