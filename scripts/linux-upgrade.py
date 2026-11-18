#!/usr/bin/env python3
"""Switch a drained Linux installation or restore its saved binary, configuration and native store."""
import argparse
import json
import os
from pathlib import Path
import select
import shutil
import subprocess
import tempfile


def cli(binary, *args):
    raw = subprocess.check_output([binary, *args], stdin=subprocess.DEVNULL)
    return json.loads(raw)["data"]


def stop(binary, socket, pid):
    # Hold a reference to this process through its full storage shutdown.
    fd = os.pidfd_open(pid)
    try:
        watcher = select.poll()
        watcher.register(fd, select.POLLIN)
        state = cli(binary, "--socket", socket, "daemon", "stop")
        if state["state"] != "draining" or not watcher.poll(120_000) or socket.exists():
            raise SystemExit("Daemon did not finish draining. Inspect the daemon log.")
    finally:
        os.close(fd)


def install(source, target):
    with tempfile.TemporaryDirectory(dir=target.parent) as tmp:
        staged = Path(tmp) / target.name
        shutil.copy2(source, staged)
        staged.replace(target)


def copy_store(source, target):
    subprocess.run(["cp", "--archive", "--reflink=auto", "--", source, target], check=True)


def upgrade(args, backup):
    app = args.app.resolve(strict=True)
    candidate = args.candidate.resolve(strict=True)
    subprocess.run([candidate, "--version"], check=True)
    flags = []
    config = Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config") / "akasha-otsuge/config.toml"
    if args.config:
        config = args.config.resolve(strict=True)
        flags.extend(["--config", config])
    if args.profile:
        flags.extend(["--profile", args.profile])
    live = cli(app, *flags, "config", "show", "--effective")
    if live["selected_profile"] != args.profile:
        raise SystemExit("Select the profile used by the running daemon.")
    paths = live["config"]["paths"]
    store, socket = Path(paths["data_dir"]).resolve(strict=True), Path(paths["socket_path"])
    config = config.resolve()
    if backup.parent != store.parent or backup == store or store.is_mount():
        raise SystemExit(f"Choose a fresh rollback directory beside the native store {store}. "
                         "A mounted store requires a filesystem-level rollback.")
    pid = cli(app, "--socket", socket, "daemon", "status")["pid"]
    proc = Path("/proc") / str(pid)
    if not app.samefile(proc / "exe"):
        raise SystemExit("The installed executable differs from the running daemon.")
    argv = (proc / "cmdline").read_bytes().rstrip(b"\0").split(b"\0")[1:]
    if b"--config" in argv:
        configured = (proc / "cwd" / os.fsdecode(argv[argv.index(b"--config") + 1])).resolve()
        if configured != config:
            raise SystemExit("Choose the configuration used by the running daemon.")
    backup.mkdir(mode=0o700)
    stop(app, socket, pid)
    shutil.copy2(app, backup / "binary")
    if config.exists():
        shutil.copy2(config, backup / "config")
    copy_store(store, backup / "store")
    manifest = dict(app=str(app), config=str(config), store=str(store), socket=str(socket),
                    config_present=config.exists(), profile=args.profile)
    (backup / "snapshot.json").write_text(json.dumps(manifest) + "\n")
    install(candidate, app)
    print(f"Installed candidate. Rollback directory: {backup}")


def restore(backup):
    saved = json.loads((backup / "snapshot.json").read_text())
    app, store, config, socket = (Path(saved[key]) for key in ("app", "store", "config", "socket"))
    if (backup / "restored").exists():
        raise SystemExit("This rollback has already completed.")
    if socket.exists():
        old = backup / "binary"
        live = cli(old, "--socket", socket, "config", "show", "--effective")
        if Path(live["config"]["paths"]["data_dir"]).resolve() != store:
            raise SystemExit("The socket now serves a different store.")
        stop(old, socket, cli(old, "--socket", socket, "daemon", "status")["pid"])
    failed = backup / "failed-store"
    if config.exists() and not (backup / "failed-config").exists():
        shutil.copy2(config, backup / "failed-config")
    with tempfile.TemporaryDirectory(dir=store.parent) as tmp:
        staged = Path(tmp) / "store"
        copy_store(backup / "store", staged)
        if not failed.exists():
            store.rename(failed)
        if not store.exists():  # Includes a retry after moving the failed store aside.
            staged.rename(store)
    install(backup / "binary", app)
    if saved["config_present"]:
        install(backup / "config", config)
    else:
        config.unlink(missing_ok=True)
    (backup / "restored").touch(exist_ok=False)
    print(f"Restored the saved installation. Post-upgrade store: {failed}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="action", required=True)
    up = commands.add_parser("upgrade")
    up.add_argument("candidate", type=Path)
    up.add_argument("backup", type=Path)
    up.add_argument("--app", type=Path, default=Path.home() / ".local/bin/akasha-otsuge")
    up.add_argument("--config", type=Path)
    up.add_argument("--profile")
    commands.add_parser("restore").add_argument("backup", type=Path)
    args = parser.parse_args()
    os.umask(0o077)
    if args.action == "upgrade":
        upgrade(args, args.backup.resolve())
    else:
        restore(args.backup.resolve(strict=True))
