# Linux binary installation

Choose the archive for your Linux architecture. Its filename includes the package version, source commit and architecture. Download the adjacent `.sha256` file into the same directory. The archive includes the full source revision in `SOURCE_COMMIT` and the compiled third-party package inventory in `THIRD-PARTY.json`.

## Install and start

Work in a fresh download directory as the daemon user. Set `archive` to the downloaded basename. Run this block in Bash for both installations and upgrades. Leave `opts` empty for the default daemon. For custom startup options, replace `opts=()` with the original selection, for example `opts=(--config /absolute/config.toml --profile work)`. Keep that selection through removal. A missing file, unexpected filename or digest mismatch stops extraction.

```bash
set -euo pipefail
sha256sum -- "$archive" | cmp - "$archive.sha256"
tar -xzf "$archive"
package=${archive%.tar.gz}
app="$HOME/.local/bin/akasha-otsuge"
opts=()
```

For a first installation, continue in the same shell. For an existing installation, go to [Upgrade and restore](#upgrade-and-restore).

```bash
install -d "$HOME/.local/bin"
install -m 0755 "$package/bin/akasha-otsuge" "$app"
"$app" --version
"$app" "${opts[@]}" daemon start
"$app" "${opts[@]}" daemon status
"$app" "${opts[@]}" doctor --json
```

The daemon uses a private Unix socket and stores its data under the XDG data directory. The archive contains an example TOML file. Use `--config /absolute/path/to/config.toml` before the subcommand when selecting your own configuration. See [daemon paths and permissions](daemon.md) and [configuration](configuration.md).

Choose one host and follow its installation and removal instructions.

| Host | Installation guide |
| --- | --- |
| Claude Code | [Session plugin, native version and authentication](adapters.md#claude-session-plugin-installation) |
| Codex | [Opt-in launcher and native hooks](adapters.md#codex-opt-in-launcher-and-native-hooks) |
| Hermes | [Private profile plugin](adapters/hermes-0.22.5/README.md) |
| Pi | [Agent directory extension](adapters/pi-1.4.0/README.md) |

The daemon starts independently of these integrations. Configure model endpoints and review settings before submitting work that needs them. See [review](review.md) and [notifications](notifications.md).

## Upgrade and restore

Verify and extract the archive above. Run the helper on Linux with Python 3.9+ and GNU coreutils, under the same user and XDG environment as the running daemon. Suspend automatic restarts and clients for the maintenance window. Use the same `app` and `opts` from the first block. Choose a fresh rollback directory next to the active native data directory.

```bash
set -euo pipefail
store=$("$app" "${opts[@]}" config show --effective | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["config"]["paths"]["data_dir"])')
backup="$(dirname -- "$store")/akasha-otsuge.rollback.$(date +%s)"
python3 "$package/scripts/linux-upgrade.py" upgrade "$package/bin/akasha-otsuge" "$backup" --app "$app" "${opts[@]}"
"$app" "${opts[@]}" daemon start
"$app" "${opts[@]}" doctor --json
```

The helper identifies the running executable and its active store, requests a normal stop, and waits up to 120 seconds for process exit. It saves the old binary, configuration (including its absence) and complete stopped store into the private rollback directory before switching the executable. GNU `cp` uses reflinks when available. Other filesystems need space for a full copy. A store mounted directly at its data directory requires a filesystem-level snapshot and restore. Keep the backup path and verified extracted package through acceptance.

If startup or validation fails, use the same package, backup path and startup options. This also works with a damaged candidate configuration. The helper drains a running candidate through its saved socket. If a daemon is serving the data directory on a different socket, stop it through that socket before restoring. The helper checks the native writer lock and restores the saved installation even if the candidate directory has been moved away. When the candidate directory exists, the helper keeps it in `failed-store`, including jobs and writes made after the snapshot. The original snapshot remains available.

```bash
set -euo pipefail
python3 "$package/scripts/linux-upgrade.py" restore "$backup"
"$app" "${opts[@]}" daemon start
"$app" "${opts[@]}" doctor --json
```

## Remove

Before removing the CLI, run the uninstall command for each integration you installed. Each command takes the same launcher, plugin directory or host home used for installation. See the [adapter guide](adapters.md). Use the same `app` and `opts` as at startup. In a new Bash shell, set those two variables first. Stop the selected daemon before removing the executable.

```bash
set -euo pipefail
"$app" "${opts[@]}" daemon stop
rm -- "$app"
```

The history and user configuration have separate paths. For moving data into a new installation, see [neutral archive export and restore](neutral-archive.md).
