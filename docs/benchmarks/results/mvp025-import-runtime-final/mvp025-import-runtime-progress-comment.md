The independent imported-runtime leaf is merged in PR #59 as `31d4c61ecf6672ae52e3b84bbd00b630bceed3f8`; its tree exactly matches reviewed `fa801c91d241582ee8e8d93dddf10f12397076ca`.

Imported stores now start with fresh local configuration, grants, job/incident queues and search state. Original memory, state, revision history and recall outputs remain readable under current permissions. Imported jobs, native tokens, invocation bindings and old delivery records cannot authorize new execution, including after later record updates. Fresh work and same-session Pi registration remain usable across restart.

- Exact unchanged `just check` PASS in 446.892s: 561 Rust passes including the installed repeat, 159 daemon cases each run, 34 Python tests, Node/fmt/Clippy and installed lifecycle/UID isolation.
- [Whole-range R1 review](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/59#issuecomment-6762120374) PASS with zero introduced findings and 60 fresh focused passes. Four import and four runtime initialization SIGKILL phases pass.
- [Exact-head hosted CI](https://github.com/RyderFreeman4Logos/akasha-otsuge/actions/runs/42777693037) SUCCESS.
- [Usage and limits](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/31d4c61ecf6672ae52e3b84bbd00b630bceed3f8/docs/neutral-archive.md) and [focused evidence](https://github.com/RyderFreeman4Logos/akasha-otsuge/tree/31d4c61ecf6672ae52e3b84bbd00b630bceed3f8/docs/benchmarks/results/mvp025-import-runtime).

Issue #25 remains OPEN for actual supported read-only mempal conversion, different-archive incremental application and complete real-sample migration acceptance. One consistent private SQLite snapshot was acquired under a kernel read-only source mount; that acquisition alone is not migration success. Controlled native protocol fixtures do not prove actual provider authentication, power-loss or real ENOSPC durability. No automatic cross-UID remapping is claimed.
