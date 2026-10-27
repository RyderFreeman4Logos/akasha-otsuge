# Provider acceptance — MVP-009

The production CLI → private daemon → durable job → HTTP client path passed on
source `986a0278d5873cb667aeb4ba82118efceb9a3666`, tree
`a17fb69214b772612e63f056444beaefb87b5557`. The release executable SHA256 is
`6b6cc71fd9dbb1d8ce904b6ad95ed4227b99a78507e0a05f493ffe387a160c3e`.
Later documentation commits do not change that source; any reuse of the manual
acceptance requires the release executable to have this exact hash.

The [archive](benchmarks/results/mvp009/README.md) contains 148 original files,
228,200 bytes, with [SHA256SUMS](benchmarks/results/mvp009/SHA256SUMS). Its manifest
SHA256 is `52393943d201e8b8e4a65e53589158bbcf6f506de331a604a68527053f632270`.
No binaries, private corpus, credentials or expanded dataset are included.

## Real elapsed time and service compatibility

The final long run launched at **2026-10-27 17:41:24.613876 UTC** and terminated at
**18:09:47.236155 UTC**, taking 1702.594 seconds. Two controlled HTTP requests
overlapped; each made one physical attempt. The actual release daemon persisted
the jobs while the accepting CLI returned immediately.

| Case | Provider elapsed | Result |
| --- | ---: | --- |
| Response delayed 1605 seconds | 1,605,030 ms | `succeeded`, complete `hello` output |
| Response delayed beyond 1700 seconds | 1,700,001 ms | `failed`, `PROVIDER_TIMEOUT` |

Both calls retain the default 1,700,000 ms provider budget, 300,000 ms queue
budget and original 2,010,000 ms workflow budget. Each workflow trace's start
equals its durable job start; the stored deadline equals start plus budget.
Connection establishment has a separate 10-second limit. Idle timeout is disabled,
automatic retries are zero, and provider execution is independent of the
10-second CLI/IPC exchange. There is no external worker/RPC/host Runner in this
path. All owned daemon/provider threads ended and scratch was removed.
See the [collected checks](benchmarks/results/mvp009/mvp009-long-r2.collected.json)
and [raw verified result](benchmarks/results/mvp009/mvp009-long-r2/verified.json).

The same release passed actual GB10 compatibility on `http://gb10:18009/v1` at
17:41:24–17:41:28 UTC:

| Endpoint/model identifier | Observed result | Provider elapsed |
| --- | --- | ---: |
| text / `abliterated-qwen-latest-27b-none` | complete `OK` | 276 ms |
| embedding / `qwen3-embedding-8b` | one finite 4096-dimensional vector | 1270 ms |
| reranker / `Querit/Querit-4B` | two raw scores: 0.466796875, 0.279296875 | 44 ms |

These are observed configured aliases, not an attestation of remote weights or
server versions. This bounded synthetic check proves protocol compatibility,
not semantic quality or long-duration behavior of GB10 itself. Its three stages
share the unchanged original 6,010,000 ms workflow budget. The
[receipt](benchmarks/results/mvp009/mvp009-gb10-r2.collected.json) retains complete
traces and executable identity. No service was restarted or reconfigured.

## Regressions, gate and review

`bash scripts/check.sh` passed on the frozen source in 82.535 seconds: 98 ordinary
Rust tests, eight Python tests, formatting, all-target Clippy, release builds and
capture acceptance. The release was separately installed in the isolated harness
and passed all 15 daemon cases, including kernel peer-UID denial with an accessible
positive control and no daemon-owned TCP listener. Manual long/GB10 cases are
excluded from these ordinary counts. [Gate receipt](benchmarks/results/mvp009/mvp009-gate2.collected.json).

The suites cover independent text/embedding/reranker endpoints, malformed and
oversized output, disconnects, slow bodies, throttling, cancellation, reload,
shutdown/crash recovery and immutable partial results. A virtual clock covers
1699-second success, the 1700-second boundary and three consecutive long stages
without shortening their automatic workflow budget.

Independent R1 review found workflow-clock renewal across configuration-restore
admission. Causal regressions first reproduced a successful expired job and an
already-expired job sending a request. The repair resumes the durable original
clock before awaiting locks and refuses expired queue/workflow admission before
physical dispatch. All three exhausted provider-admission controls now send zero
requests. The [repair index](benchmarks/results/mvp009/mvp009-r1-fix-evidence.json)
preserves the failed fixture setup separately from behavioral RED/GREEN.
Independent [R2 review](benchmarks/results/mvp009/mvp009-review-r2.md) covers the
whole 17-file candidate and reports **PASS, zero actionable findings**.

Historical `69853f1`/release `0596d114…` long and GB10 receipts remain archived as
`r1`. They predate the workflow repair and are not substituted for the final `r2`
receipts. Compilation/harness failures in the focused evidence index are retained
and are never counted as successful behavior tests.

Environment: Debian 12, Linux `6.1.0-52-amd64` x86_64,
Rust `1.99.1 (58cf8f9c3 2026-10-13)`. Exact commands are in each admission receipt;
[provider documentation](providers.md) gives configuration and explicit rerun
commands. Final publication, exact documentation-candidate gate/review and CI are
recorded in [Issue #9](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/9).
Host CLI Runner long tests and retrieval quality belong to their dependent Issues.
