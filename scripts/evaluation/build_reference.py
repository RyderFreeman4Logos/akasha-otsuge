"""Build isolated H01 observers from the frozen tree using an existing Cargo target directory."""

import argparse
import hashlib
import json
import os
import shutil
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
BASE = "a177d6e23568b627ec9eef593d88289bc8634041"


def sha(path):
    with path.open("rb") as data:
        return hashlib.file_digest(data, "sha256").hexdigest()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--output", required=True, type=Path, help="new private directory on SSD"
    )
    p.add_argument(
        "--target-dir", required=True, type=Path, help="reusable Cargo build cache"
    )
    p.add_argument(
        "--prepare-only",
        action="store_true",
        help="export and validate Cargo targets without compiling",
    )
    args = p.parse_args()
    os.umask(0o077)
    output = args.output.resolve()
    output.mkdir(mode=0o700)
    tree = output / "source"
    tree.mkdir()
    archive = output / "source.tar"
    subprocess.run(
        [
            "git",
            "archive",
            "--output",
            str(archive),
            BASE,
            "src",
            "plugins",
            "examples",
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            "README.md",
        ],
        cwd=ROOT,
        check=True,
    )
    subprocess.run(["tar", "xf", str(archive), "-C", str(tree)], check=True)
    archive.unlink()
    fixtures = tree / "src/daemon/jobs"
    fixtures.mkdir(exist_ok=True)
    shutil.copyfile(HERE / "reference_service.rs", fixtures / "reference_fixture.rs")
    with (tree / "src/daemon/jobs.rs").open("a") as f:
        f.write("\npub mod reference_fixture;\n")
    with (tree / "src/daemon.rs").open("a") as f:
        f.write("\npub use jobs::reference_fixture;\n")
    binaries = tree / "src/bin"
    binaries.mkdir(exist_ok=True)
    inputs = {
        "reference-read": "reference_read.rs",
        "reference-service": "reference_service_main.rs",
    }
    for name, source in inputs.items():
        shutil.copyfile(HERE / source, binaries / (name + ".rs"))
    meta = json.loads(
        subprocess.check_output(
            [
                "cargo",
                "metadata",
                "--format-version",
                "1",
                "--locked",
                "--offline",
                "--no-deps",
            ],
            cwd=tree,
        )
    )
    targets = {
        t["name"]
        for package in meta["packages"]
        for t in package["targets"]
        if t["kind"] == ["bin"]
    }
    assert set(inputs) <= targets
    if args.prepare_only:
        return
    target = args.target_dir.resolve()
    command = ["cargo", "build", "--locked", "--offline", "--target-dir", str(target)]
    for name in inputs:
        command.extend(["--bin", name])
    subprocess.run(command, cwd=tree, check=True)
    for name, source in inputs.items():
        dest = output / name
        shutil.copy2(target / "debug" / name, dest)
        receipt = {
            "base": BASE,
            "compiled_library_commit": BASE,
            "binary_sha256": sha(dest),
            "source_sha256": sha(
                HERE
                / ("reference_service.rs" if name == "reference-service" else source)
            ),
            "main_sha256": sha(HERE / source),
            "daemon_rs_sha256": sha(tree / "src/daemon.rs"),
            "jobs_rs_sha256": sha(tree / "src/daemon/jobs.rs"),
            "build": "Cargo --locked --offline",
        }
        (output / (name + ".json")).write_text(json.dumps(receipt, indent=2) + "\n")
    shutil.rmtree(tree)


if __name__ == "__main__":
    main()
