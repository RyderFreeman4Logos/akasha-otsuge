"""Run the nine-step package rehearsal in a fresh Pi-only Linux namespace."""

import argparse
import os
import subprocess
from pathlib import Path


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ("archive", "pi-install", "node", "output-directory"):
        p.add_argument("--" + name, type=Path, required=True)
    for name in ("commit", "sha256"):
        p.add_argument("--" + name, required=True)
    args = p.parse_args()
    os.umask(0o077)
    archive = args.archive.resolve(strict=True)
    checksum = archive.with_name(archive.name + ".sha256")
    output = args.output_directory.resolve()
    suite = Path(__file__).resolve().parents[1] / "tests/fixtures"
    node = args.node.resolve(strict=True)
    pi = args.pi_install.resolve(strict=True)
    if not (pi / "node_modules/.bin/pi").is_file():
        p.error("--pi-install must contain Pi 1.4.0 at node_modules/.bin/pi")
    if checksum.read_text().split() != [args.sha256, archive.name]:
        p.error("the sibling checksum must match the supplied digest and archive name")
    output.mkdir(mode=0o700)
    cmd = [
        "bwrap",
        "--die-with-parent",
        "--new-session",
        "--unshare-all",
        "--ro-bind",
        "/usr",
        "/usr",
        "--symlink",
        "usr/bin",
        "/bin",
        "--symlink",
        "usr/lib",
        "/lib",
        "--symlink",
        "usr/lib64",
        "/lib64",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        "--tmpfs",
        "/tmp",
        "--dir",
        "/opt",
        "--dir",
        "/tools",
        "--dir",
        "/dist",
        "--dir",
        "/helper",
        "--ro-bind",
        str(archive),
        "/dist/" + archive.name,
        "--ro-bind",
        str(checksum),
        "/dist/" + checksum.name,
        "--ro-bind",
        str(pi),
        "/opt/pi",
        "--ro-bind",
        str(node),
        "/tools/node",
        "--ro-bind",
        str(suite / "pi/1.4.0"),
        "/fixture",
        "--ro-bind",
        str(suite / "mempal21-28.sql"),
        "/mempal.sql",
        "--ro-bind",
        str(Path(__file__).with_name("check_linux_upgrade.py")),
        "/helper/check_linux_upgrade.py",
        "--bind",
        str(output),
        "/case",
        "--clearenv",
        "--setenv",
        "HOME",
        "/tmp",
        "--setenv",
        "PATH",
        "/tools:/usr/bin:/bin",
        "--setenv",
        "PYTHONPATH",
        "/helper",
        "--chdir",
        "/case",
        "/usr/bin/python3",
        "-B",
        "/fixture/check_release_rehearsal.py",
        "--pi",
        "/opt/pi/node_modules/.bin/pi",
        "--archive",
        "/dist/" + archive.name,
        "--commit",
        args.commit,
        "--sha256",
        args.sha256,
        "--mempal-schema",
        "/mempal.sql",
        "--scratch",
        "/case",
        "--output",
        "/case/result.json",
    ]
    subprocess.run(cmd, check=True, timeout=600)


if __name__ == "__main__":
    main()
