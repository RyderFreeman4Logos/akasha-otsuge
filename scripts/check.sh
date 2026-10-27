#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
python3 scripts/check_plan.py
python3 -m unittest discover -s scripts -p 'test_*.py'
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked
cargo build --example reconcile --release --locked
python3 scripts/check_capture_command.py target/release/examples/reconcile
python3 scripts/check_daemon_command.py target/release/akasha-otsuge --install
target/release/akasha-otsuge --help > /dev/null
target/release/akasha-otsuge --version
git diff --check
