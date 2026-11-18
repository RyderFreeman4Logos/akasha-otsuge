# Interactive capture on the released package

Each of the four native hosts ran in a controlling terminal against the Linux package from `4e7b3a2`. Each namespace exposed only the selected host CLI. Local deterministic protocol responses exercised the native programs and the production daemon with synthetic input. The table identifies what each terminal established. [Per-host measurements](interactive-hosts-r1.json) · [Package workflow](https://github.com/RyderFreeman4Logos/akasha-otsuge/actions/runs/44311528619)

| Host | Input and durable result |
| --- | --- |
| Claude Code 2.1.311 | The production plugin captured the initial TUI turn, resume, manual compaction and post-compaction input. The release-matched Caller script checked native transcript UUIDs, captured bytes and durable checkpoints. This script isolates Caller capture. The separate single-host workflow exercises the independent Claude Runner. |
| Codex 0.164.0 | A typed short-hint turn in the opt-in launcher invoked Consult. The native Codex Runner read the original, and the returned bytes, scope, fence and final receipt matched. The TUI exited zero. |
| Hermes 0.22.5, 4960e41 | A typed no-hint turn in native `hermes chat --tui` used the installed plugin. The native Hermes Runner independently retrieved the source. The completed transcript turn, saved invocation, bound scope, final receipt and original text agreed. The TUI exited zero. |
| Pi 1.4.0 | A typed no-hint turn in the normal Pi TUI invoked its extension. The native Pi Runner read the fenced original and returned the matching receipt. The TUI exited zero. |

The historical Pi 1.4.0 Caller check expected a binding in the tool reply. The released CLI returns an asynchronous job acknowledgement with `job_id`, `receipt_id` and `status`. Replaying the old TUI script against the package raised `KeyError: caller`. The package replay below follows the returned job through the production daemon and checks the exact original bytes. The earlier Codex and Hermes startup probes reached the UI while the observer waited for an older readiness prompt or a sharing preference. The final harness checks the current ready state and configures the fresh Hermes profile with sharing disabled. One earlier Claude archive run exited before producing a receipt. Its subsequent isolated reruns completed. No root cause was established for that intermittent probe failure. [Earlier single-host evidence](isolated-workflows-r1.md) · [Real Hermes inference observation](hermes-loopback-r1.md)

## Replay

Use the release and host path variables from the matrix, along with `fd`, `rg`, the compiled Hermes TUI distribution, and a repository containing the exact package commit. The Claude command obtains the two release-matched Caller fixture files from that commit. The remaining commands start a native Caller in a controlling terminal and a separate native Runner. All four use private local test servers. [Matrix prerequisites](native-host-matrix-r1.md#replay) · [Replay sources](interactive-host-replay-r1.tar.gz)

```bash
mkdir -m 700 "$SCRATCH/terminals"
tar -xzf interactive-host-replay-r1.tar.gz -C "$SCRATCH/terminals"
base=(--product "$AKASHA" --scratch "$SCRATCH")
tools=(--fd "$(command -v fd)" --rg "$(command -v rg)")
python3 -B "$SCRATCH/terminals/claude.py" "${base[@]}" --claude "$CLAUDE" --repository "$AKASHA_REPOSITORY" --output "$SCRATCH/tui-claude.json" || exit 1
python3 -B "$SCRATCH/terminals/run.py" codex codex short "${base[@]}" "${tools[@]}" --codex "$CODEX" --codex-catalog "$CODEX_CATALOG" --output "$SCRATCH/tui-codex.json" || exit 1
python3 -B "$SCRATCH/terminals/run.py" hermes hermes none "${base[@]}" "${tools[@]}" --node "$NODE" --hermes-owner "$HERMES_OWNER" --hermes-install "$HERMES_INSTALL" --hermes-tui-dist "$HERMES_TUI_DIST" --output "$SCRATCH/tui-hermes.json" || exit 1
python3 -B "$SCRATCH/terminals/run.py" pi pi none "${base[@]}" "${tools[@]}" --pi-root "$PI_ROOT" --node "$NODE" --output "$SCRATCH/tui-pi.json" || exit 1
```
