# Development checkpoint — 2026-10-23

Repository: https://github.com/RyderFreeman4Logos/akasha-otsuge (public, Apache-2.0).
All 41 source pages and all 28 real Issues/dependency mappings are preserved.

- MVP-001 / #1: complete. PR #29 merged as `9c60f50022a4d5b29338e8603e08153037926856`; [clean Linux CI](https://github.com/RyderFreeman4Logos/akasha-otsuge/actions/runs/39879774711) passed. Local gate: 4 Rust CLI tests, 6 Python plan tests, formatting, Clippy and release smoke.
- MVP-002 / #2: complete. PR #30 merged as `349431a350e9e4ae3364b126c4d2a7133dd37d22`; [clean Linux CI](https://github.com/RyderFreeman4Logos/akasha-otsuge/actions/runs/39881917776) passed. Exact-commit local gate includes 17 configuration, 4 CLI and 6 Python tests, formatting, Clippy and release smoke. Evidence and limits are recorded on #2.
- Active: MVP-003 / #3. [Storage criteria](../benchmarks/storage-contract.md) frozen before candidate measurement. Helix embedded first, specified fallback only after hard failure. No production backend has been chosen or scale acceptance claimed. The [100,000-change historical oracle](../benchmarks/results/helix-authority-100k.json) passes after separate-process reopen; full corpus/concurrency/recovery acceptance remains open.

[Complete remaining checklist and restart review](todo.md): 26 Issues remain open.
The interrupted 38,462-vector index recovered and activated; [24 real search
checks](../benchmarks/results/helix-real-search.json) pass across eight partitions
after process reopen. Full corpus preparation is independent: its first Python
run reached 1,000,000 vector records then exceeded a 2 GiB preparation cgroup.
The read-only preparation was restarted with 8 GiB; engine acceptance budgets
remain 512 MiB and 2 GiB. No full-size pass follows from these diagnostics.
The remaining MVP Issues are open. Library configuration tests are not provider,
daemon, four-host, 1600–1690s or 50GB/100-client acceptance. Persistent configuration
audit, daemon commands and file watcher integration remain assigned to MVP-008;
actual notifier dispatch is MVP-023. Installed user tools and running sessions have
not been changed.

Use `bash scripts/check.sh` (or `just check`) on the current candidate. Issue closure
requires its specific evidence, source review and clean Linux CI. Continue one main
Issue at a time and preserve the original requirement coverage.
