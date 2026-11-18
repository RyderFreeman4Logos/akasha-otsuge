# Linux binary installation

Choose the archive for your Linux architecture. Its filename includes the package version, source commit and architecture. Download the adjacent `.sha256` file into the same directory. The archive includes the full source revision in `SOURCE_COMMIT` and the compiled third-party package inventory in `THIRD-PARTY.json`.

## Install and start

Set `archive` to the downloaded filename. Run the following commands as the user who will run the daemon.

```sh
sha256sum -c "$archive.sha256"
tar -xzf "$archive"
package=${archive%.tar.gz}
install -d "$HOME/.local/bin"
install -m 0755 "$package/bin/akasha-otsuge" "$HOME/.local/bin/akasha-otsuge"
app="$HOME/.local/bin/akasha-otsuge"
"$app" --version
"$app" daemon start
"$app" daemon status
"$app" doctor --json
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

## Upgrade and remove

Verify and extract the new archive first. Keep the old executable and the current configuration until you have checked the new installation. The stop command acknowledges draining. The daemon releases its Unix socket after closing storage. Set `socket` to the path used by this installation. The commands below use the default. The wait exits after 120 seconds if shutdown is still in progress. Inspect the private daemon log before continuing in that case. Use the same configuration and profile on restart.

```sh
set -e
socket="${XDG_RUNTIME_DIR:-$HOME/.local/run}/akasha-otsuge/service.sock"
"$app" daemon stop
timeout 120 sh -c 'while test -S "$1"; do sleep 0.2; done' sh "$socket"
install -m 0755 "$package/bin/akasha-otsuge" "$app.new"
mv -f "$app.new" "$app"
"$app" daemon start
"$app" doctor --json
```

For a backup, copy your configuration and export the stopped store to a fresh private directory. See [backup and restore commands](neutral-archive.md).

Before removing the CLI, run the uninstall command for each integration you installed. Each command takes the same launcher, plugin directory or host home used for installation. See the [adapter guide](adapters.md). Then stop the daemon and remove the executable you installed.

```sh
"$app" daemon stop
rm -- "$app"
```

The history and user configuration have separate paths. For moving data into a new installation, see [neutral archive export and restore](neutral-archive.md).
