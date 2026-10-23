# Development checkpoint — 2026-10-22

Repository: https://github.com/RyderFreeman4Logos/akasha-otsuge (public, Apache-2.0).
All 41 source pages and all 28 real Issues/dependency mappings are preserved.

- MVP-001 / #1: complete. PR #29 merged as `9c60f50022a4d5b29338e8603e08153037926856`; [clean Linux CI](https://github.com/RyderFreeman4Logos/akasha-otsuge/actions/runs/39879774711) passed. Local gate: 4 Rust CLI tests, 6 Python plan tests, formatting, Clippy and release smoke.
- MVP-002 / #2: implementation candidate on `feat/mvp-002-config`. Configuration resolution, resource preparation, whole-snapshot reload, pending restart, live delivery checks and deadline contracts implemented; 17 configuration tests pass. Exact-commit gate, PR/CI and final closure are recorded on #2.
- Next: MVP-003 / #3, after #2 acceptance. Read `docs/spec/05.md`, `08.md` and `issues/MVP-003.md`; freeze the storage benchmark/criteria before experimenting. Helix embedded first, specified fallback only after hard failure. No production backend has been chosen.

The remaining MVP Issues are open. Library configuration tests are not provider,
daemon, four-host, 1600–1690s or 50GB/100-client acceptance. Persistent configuration
audit, daemon commands and file watcher integration remain assigned to MVP-008;
actual notifier dispatch is MVP-023. Installed user tools and running sessions have
not been changed.

Use `bash scripts/check.sh` (or `just check`) on the current candidate. Issue closure
requires its specific evidence, source review and clean Linux CI. Continue one main
Issue at a time and preserve the original requirement coverage.
