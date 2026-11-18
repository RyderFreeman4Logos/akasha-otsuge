#!/usr/bin/env python3
"""Switch a drained Linux installation or restore its saved binary, configuration and native store."""
import argparse
from contextlib import ExitStack, contextmanager
import ctypes
import fcntl
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


def daemon_config(proc):
    argv = (proc / "cmdline").read_bytes().rstrip(b"\0").split(b"\0")[1:]
    explicit = None
    for flag, value in zip(argv[::2], argv[1::2]):
        if flag not in (b"--config", b"--socket", b"--profile"):
            break
        if flag == b"--config":
            explicit = os.fsdecode(value)
    if explicit is not None:
        return (proc / "cwd" / explicit).resolve()
    dirs = {}
    for item in (proc / "environ").read_bytes().split(b"\0"):
        key, _, value = item.partition(b"=")
        if key in (b"HOME", b"XDG_CONFIG_HOME") and os.path.isabs(value):
            dirs[key] = Path(os.fsdecode(value))
    config_home = dirs.get(b"XDG_CONFIG_HOME") or dirs[b"HOME"] / ".config"
    return (config_home / "akasha-otsuge/config.toml").resolve()


@contextmanager
def exclusive(path, *, directory=False):
    flags = os.O_RDONLY | os.O_NOFOLLOW | (os.O_DIRECTORY if directory else os.O_NONBLOCK)
    fd = os.open(path, flags)
    try:
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise SystemExit(f"Still in use: {path}") from None
        yield fd
    finally:
        os.close(fd)


@contextmanager
def stopped_store(path):
    # The directory protects native opens; the engine lock remains held by an open writer.
    with exclusive(path, directory=True) as fd:
        with exclusive(Path(f"/proc/self/fd/{fd}/authority/kv/LOCK")):
            if not os.path.samestat(os.fstat(fd), path.stat()):
                raise SystemExit(f"Store directory changed: {path}")
            yield


def publish(source, target, *, exchange):
    # A single Linux rename preserves the candidate and never replaces an unexpected arrival.
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.renameat2(-100, os.fsencode(source), -100, os.fsencode(target), 2 if exchange else 1):
        code = ctypes.get_errno()
        raise OSError(code, os.strerror(code), str(target))


def upgrade(args, backup):
    app = args.app.resolve(strict=True)
    candidate = args.candidate.resolve(strict=True)
    subprocess.run([candidate, "--version"], check=True)
    flags = []
    if args.config:
        flags.extend(["--config", args.config.resolve(strict=True)])
    if args.profile:
        flags.extend(["--profile", args.profile])
    live = cli(app, *flags, "config", "show", "--effective")
    if live["selected_profile"] != args.profile:
        raise SystemExit("Select the profile used by the running daemon.")
    paths = live["config"]["paths"]
    store, socket = Path(paths["data_dir"]).resolve(strict=True), Path(paths["socket_path"])
    if backup.parent != store.parent or backup == store or store.is_mount():
        raise SystemExit(f"Choose a fresh rollback directory beside the native store {store}. "
                         "A mounted store requires a filesystem-level rollback.")
    pid = cli(app, "--socket", socket, "daemon", "status")["pid"]
    proc = Path("/proc") / str(pid)
    if not app.samefile(proc / "exe"):
        raise SystemExit("The installed executable differs from the running daemon.")
    config = daemon_config(proc)
    if args.config and config != args.config.resolve(strict=True):
        raise SystemExit("Choose the configuration used by the running daemon.")
    backup.mkdir(mode=0o700)
    stop(app, socket, pid)
    shutil.copy2(app, backup / "binary")
    if config.exists():
        shutil.copy2(config, backup / "config")
    with stopped_store(store):
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
    identity = backup / "restore-identity.json"
    with ExitStack() as locks:
        present = store.exists()
        if present:
            locks.enter_context(stopped_store(store))
        if config.exists() and not (backup / "failed-config").exists():
            install(config, backup / "failed-config")
        if not identity.exists():
            with tempfile.TemporaryDirectory(dir=backup) as tmp:
                staged = Path(tmp) / "store"
                copy_store(backup / "store", staged)
                meta = staged.stat()
                record = Path(tmp) / "identity"
                record.write_text(json.dumps([meta.st_dev, meta.st_ino]))
                # An interruption before moving the staged directory leaves the live store intact.
                record.replace(identity)
                publish(staged, failed, exchange=False)
        expected = json.loads(identity.read_text())
        live = store.stat() if present else None
        if live is None or [live.st_dev, live.st_ino] != expected:
            if not failed.exists():
                identity.unlink()
                raise SystemExit("Snapshot staging was interrupted. Run restore again.")
            locks.enter_context(stopped_store(failed))
            meta = failed.stat()
            if [meta.st_dev, meta.st_ino] != expected:
                raise SystemExit("Restore directories changed. Inspect the preserved snapshot.")
            publish(failed, store, exchange=present)
        install(backup / "binary", app)
        if saved["config_present"]:
            install(backup / "config", config)
        else:
            config.unlink(missing_ok=True)
        (backup / "restored").touch(exist_ok=False)
    print(f"Restored the saved installation from {backup}.")


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
        backup = args.backup.resolve(strict=True)
        with exclusive(backup, directory=True):
            restore(backup)
