# Isolated native workflows

Each pinned host supplied both the Caller and the independent Runner. The other three CLIs were absent. All four runs on the published Linux package completed Consult, retrieved the original, extracted a scoped proposal and ran the three Auto-Grill roles through the native executor. Each generated two Auto-Grill candidates. Every candidate retained its machine-generated provenance and had no human approval. The operator moved the extraction review from open to investigating and acknowledged a locally delivered notification twice with the same generation and key. Both doctor commands exited successfully and returned JSON. The four workflows passed again from the replay archive. [Per-host receipts](isolated-workflows-r1.json) · [Eight native Consult routes](native-host-matrix-r1.md) · [Package workflow](https://github.com/RyderFreeman4Logos/akasha-otsuge/actions/runs/44311528619)

| Native host | Isolated Consult on this package | Earlier interactive capture with other CLIs absent | Inference in that interactive run |
| --- | --- | --- | --- |
| Claude Code 2.1.311 | Both hints, explicit 45 s executor budget | Production hook captured initial, resumed and post-compaction turns in a controlling terminal. Native transcript hashes and checkpoints matched. | Local deterministic Anthropic protocol |
| Codex 0.164.0 | Both hints | Two simultaneous controlling terminals returned distinct invocations, cursors and native transcript hashes. | Existing ChatGPT login |
| Hermes 0.22.5, 4960e41 | Both hints | Two native chat terminals submitted distinct invocations. Both profiles persisted the capture. | Existing Codex-backed Hermes provider |
| Pi 1.4.0 | Both hints | Two simultaneous terminals, a resumed session and a post-compaction turn produced four fenced bindings and completed capture jobs. | Previously authenticated local router, Grok 4.7 |

The interactive runs used earlier product builds. Their private receipt hashes are in the summary. The historical scripts and published native observations identify the native versions, captured records and isolation checks. [Claude Caller and controlling terminal](../../../../scripts/check-claude-caller.py) · [Codex receipt](../../../adapters/codex-0.164.0/acceptance.json) · [Hermes receipt](../../../adapters/hermes-0.22.5/tui-acceptance.json) · [Pi native controlling terminal](../../../../tests/fixtures/pi/1.4.0/check_native_tui.py)

The four new runs used a deterministic local protocol and synthetic source text with zero external inference requests. They validate wiring, source bytes, authority, review and delivery. Claude subscription quality and the blinded 60-task evaluation remain open. The first Pi attempt returned a typed protocol error and queued no Consult. The isolated rerun and the archived replay succeeded. The evidence does not establish the cause of that first failure.

## Replay the four native workflows

Use the Linux tools, unpacked release and host path variables from the matrix replay. The commands start fresh private PID, mount and network namespaces. They write one receipt per host. [Prerequisites](native-host-matrix-r1.md#replay) · [Python sources](isolated-workflow-replay-r1.tar.gz)

```bash
mkdir -m 700 "$SCRATCH/workflows"
tar -xzf isolated-workflow-replay-r1.tar.gz -C "$SCRATCH/workflows"
base=(--product "$AKASHA" --scratch "$SCRATCH")
python3 -B "$SCRATCH/workflows/claude.py" claude claude none "${base[@]}" --claude "$CLAUDE" --output "$SCRATCH/workflow-claude.json" || exit 1
python3 -B "$SCRATCH/workflows/run.py" codex codex short "${base[@]}" --codex "$CODEX" --codex-catalog "$CODEX_CATALOG" --output "$SCRATCH/workflow-codex.json" || exit 1
python3 -B "$SCRATCH/workflows/run.py" hermes hermes none "${base[@]}" --hermes-owner "$HERMES_OWNER" --hermes-install "$HERMES_INSTALL" --output "$SCRATCH/workflow-hermes.json" || exit 1
python3 -B "$SCRATCH/workflows/run.py" pi pi none "${base[@]}" --pi-root "$PI_ROOT" --node "$NODE" --output "$SCRATCH/workflow-pi.json" || exit 1
```
