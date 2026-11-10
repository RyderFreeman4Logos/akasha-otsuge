The first independent MVP-024 leaf is merged in PR #56 as `c059fda030e1c7f5d4ec922f793088450c6bc4f0`; its tree exactly matches reviewed `ffc279fe7aaf7aa1ac9e6d3160926b90c17abaca`.

This adds compatible target/peak/service budget configuration and on-demand Linux RSS/PSS observations for the daemon, guarded workers and detached descendants. Status retains the engine's actual opening budget across reload. It explicitly reports that software memory admission and managed cgroup enforcement are not installed yet.

- Unmodified `just check` PASS in 453.478s: 541 Rust passes including installed repeat, 157 daemon tests per run, 34 Python tests, Clippy/format/Node, installation/lifecycle/UID checks and zero TCP listeners.
- [Independent whole-range R1 review](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/56#issuecomment-6758757105) PASS, zero introduced findings; 24 config, 3 unit and 2 real daemon focused tests pass.
- [Exact-head hosted CI](https://github.com/RyderFreeman4Logos/akasha-otsuge/actions/runs/42753418883) SUCCESS with restored/hit Cargo cache.
- [Budget/status contract](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/c059fda030e1c7f5d4ec922f793088450c6bc4f0/docs/memory-resources.md) and [causal evidence](https://github.com/RyderFreeman4Logos/akasha-otsuge/tree/c059fda030e1c7f5d4ec922f793088450c6bc4f0/docs/benchmarks/results/mvp024-observability).

Issue #24 remains OPEN: admission/draining/cgroups, continuous peak accounting, verified corpus and production index coverage, 100-client mixed load, both full memory profiles and fault/recovery oracles are still required. The next independent work is #25's lossless import/export so the full-data campaign can reuse a production data path. No product-scale acceptance is claimed by these fixtures.
