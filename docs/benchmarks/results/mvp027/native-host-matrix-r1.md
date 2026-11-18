# Native Caller and Runner matrix

The four native CLIs completed a directed cycle on Linux x86_64. Each caller submitted a Consult using its installed integration. Each named native runner read captured caller content and returned a cited result. The stored invocation matched the runner origin and fence, and the public result, receipt and retrieved original agreed. These eight runs used deterministic loopback protocols in isolated PID, mount and network namespaces. They incurred zero external inference calls. [Machine-readable observations](native-host-matrix-r1.json) · [Published package](https://github.com/RyderFreeman4Logos/akasha-otsuge/actions/runs/44311528619)

| Caller ↓ / Runner → | Claude Code 2.1.311 | Codex 0.164.0 | Hermes 0.22.5 / 4960e41 | Pi 1.4.0 |
| --- | --- | --- | --- | --- |
| Claude Code | Isolated, short hint¹ | Cycle, short hint | Contract | Contract |
| Codex | Contract | Isolated, no hint | Cycle, short hint | Earlier native empty-result case |
| Hermes | Contract | Contract | Isolated, short hint | Cycle, no hint |
| Pi | Cycle, no hint¹ | Earlier native model run | Contract | Isolated, short hint |

Every cell passed the same external Rust API contract. It supplies a private marker for each caller, constructs the production scoped context, binds the origin and configuration generation and collects the terminal output. The local `just check` run at `7a9c073` and the hosted CI job at `84b6519` both recorded the named test as passing. The diagonal cases additionally checked that the other three native CLIs were absent. The earlier Pi→Codex run used a native model, and the earlier Codex→Pi empty-result case used a deterministic transport. [Contract source](../../../../tests/support/adapter_matrix.rs#L115) · [Hosted job](https://github.com/RyderFreeman4Logos/akasha-otsuge/actions/runs/44323738249/job/134869300156) · [Earlier native model observations](../mvp016/README.md)

¹ The released binary completed the Claude Runner cases with a 45-second executor budget. At the default 1,700 seconds, the first local context-tool read failed. A separate native diagnostic isolated the Claude Code 2.1.311 subprocess timeout limit. A 600,000-ms timeout succeeded and 600,001 ms failed. The hosted package SHA-256 remains the same for both product controls.

## Scope of the first eight runs

The subsequent isolated workflows completed the second hint mode, native extraction, Auto-Grill, review, local notification and configuration diagnostics on each host. The report also identifies earlier interactive captures. [Follow-up workflows and replay](isolated-workflows-r1.md)

| Capability observed with other CLIs absent | Claude Code | Codex | Hermes | Pi |
| --- | --- | --- | --- | --- |
| Normal capture and standalone native runner | Plugin / print CLI | Opt-in hook / exec CLI | Plugin / one-shot native chat | Extension / native print CLI |
| Consult hints in isolated runs | Short | None | Short | Short and none |
| Original readback and terminal receipt | Passed | Passed | Passed | Passed |
| Extraction, review and delivery | Pending | Pending | Pending | An independent package run produced a sourced candidate, an investigating review and idempotent delivery acknowledgement. It used a local extractor endpoint. |
| Auto-Grill on the isolated host | Pending | Pending | Pending | Pending |
| Interactive TUI capture and configuration diagnostics in this run | Pending | Pending | Pending | Package doctor and host doctor passed. TUI capture pending. |

Earlier authenticated Codex, Hermes and Pi runs have their own provenance. Claude subscription inference remains open. Blind quality scoring remains separate for the 60 frozen tasks. [Frozen single-host requirements](../../../spec/09.md) · [Codex native history](../../../adapters/codex-0.164.0/acceptance.json) · [Hermes native history](../../../adapters/hermes-0.22.5/README.md) · [Claude subscription tracking](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/12)

## Replay

Use a writable private scratch directory on Linux, Python 3, bubblewrap and the unpacked binary from the linked package. Its executable SHA-256 is `8c0114b2803df101fcebb934a9c1bd170abf67b522b5775c820b14e69c1c1133`. All eight commands below passed again from the archived scripts. Extract the included Python sources and set the path variables below for the native installations under test. The Codex catalog input supplies model metadata. The harness copies one public model entry into the isolated home. The Hermes options select a local source clone, the managed installation with its `facts.json` and the associated versioned tools. The private namespace builds a fresh Hermes profile and writable leases; the local account's profiles and credentials are outside it. The Claude executable and Pi installation must match the recorded versions. [Replay sources](native-matrix-replay-r1.tar.gz)

```bash
mkdir -m 700 "$SCRATCH/replay"
tar -xzf native-matrix-replay-r1.tar.gz -C "$SCRATCH/replay"
common=(--product "$AKASHA" --scratch "$SCRATCH" --pi-root "$PI_ROOT" --node "$NODE" --codex "$CODEX" --codex-catalog "$CODEX_CATALOG")
managed=(--hermes-owner "$HERMES_OWNER" --hermes-install "$HERMES_INSTALL")
for triple in 'codex hermes short' 'hermes pi none' 'codex codex none' 'hermes hermes short' 'pi pi short'; do
  read -r caller runner hint <<< "$triple"
  python3 -B "$SCRATCH/replay/run.py" "$caller" "$runner" "$hint" "${common[@]}" "${managed[@]}" --output "$SCRATCH/$caller-$runner.json" || exit 1
done
for triple in 'pi claude none' 'claude codex short' 'claude claude short'; do
  read -r caller runner hint <<< "$triple"
  python3 -B "$SCRATCH/replay/claude.py" "$caller" "$runner" "$hint" "${common[@]}" --claude "$CLAUDE" --output "$SCRATCH/$caller-$runner.json" || exit 1
done
```
