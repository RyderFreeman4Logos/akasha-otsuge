# Linux binary installation

Choose the archive for your Linux architecture. Its filename includes the package version, source commit and architecture. Download the adjacent `.sha256` file into the same directory. A release's support matrix identifies the platforms and host versions exercised on that build.

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

Choose the integration for your installed CLI in the [adapter guide](adapters.md). It covers Claude Code, Codex, Hermes and pi. The daemon starts independently of these integrations. Configure model endpoints and review settings before submitting work that needs them. See [review](review.md) and [notifications](notifications.md).

## Upgrade and remove

Verify and extract the new archive first. Keep the old executable and the current configuration until you have checked the new installation. Stop the daemon, replace the executable at the same path, then start it using the same configuration. Replacing an executable while an old process is running leaves that process on its old version.

```sh
"$app" daemon stop
install -m 0755 "$package/bin/akasha-otsuge" "$app.new"
mv -f "$app.new" "$app"
"$app" daemon start
"$app" doctor --json
```

Before removing the CLI, run the uninstall command for each integration you installed. Each command takes the same launcher, plugin directory or host home used for installation. See the [adapter guide](adapters.md). Then stop the daemon and remove the executable you installed.

```sh
"$app" daemon stop
rm -- "$app"
```

The history and user configuration have separate paths. For moving data into a new installation, see [neutral archive export and restore](neutral-archive.md).
