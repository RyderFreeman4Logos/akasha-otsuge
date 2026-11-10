The first independent MVP-025 export leaf is merged in PR #57 as `3b6bf74d57b275e154195571700d64406832c2d3`; its tree exactly matches reviewed `7ad2aab5721a2f840865da8a77d9bb367b6d9909`.

The public offline `archive export` command preserves original bytes, all four record kinds, revisions, relations, commit clocks and original receipts in JSONL plus a blob stream. It creates a private new destination, checks source ownership and publishes the manifest after both data files are synced. No old jobs or configuration are activated by export.

- Unmodified `just check` PASS in 511.619s: 545 Rust passes including installed repeat, 157 daemon cases per run, 34 Python tests, Node/format/Clippy and installed lifecycle/UID checks.
- [Independent whole-range R1 review](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/57#issuecomment-6759459288) PASS with zero introduced findings; four archive, five CLI and 17 storage tests pass.
- [Exact-head hosted CI](https://github.com/RyderFreeman4Logos/akasha-otsuge/actions/runs/42758677032) SUCCESS with restored/hit Cargo cache.
- [Archive format and usage](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/3b6bf74d57b275e154195571700d64406832c2d3/docs/neutral-archive.md) and [causal evidence](https://github.com/RyderFreeman4Logos/akasha-otsuge/tree/3b6bf74d57b275e154195571700d64406832c2d3/docs/benchmarks/results/mvp025-neutral-export).

Issue #25 remains OPEN: validating import, a supported read-only mempal reader, a verified real source sample, duplicate/incremental application, interrupted recovery and effective-state/history/receipt round-trip acceptance remain required. EFBIG coverage is not an ENOSPC or power-loss claim. Source-only mempal discovery does not count as migration acceptance.
