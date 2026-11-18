#!/usr/bin/env python3
"""Build a native Linux package from this checkout and print its SHA-256."""
import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import platform
import subprocess
import tarfile
import tempfile
import tomllib


ROOT = Path(__file__).resolve().parents[1]


def output(*command):
    return subprocess.check_output(command, cwd=ROOT, text=True).strip()


def add(archive, name, stream, size, mode, timestamp):
    header = tarfile.TarInfo(name)
    header.size = size
    header.mode = mode
    header.mtime = timestamp
    archive.addfile(header, stream)


def dependency_notices(compiled):
    host = next(line.removeprefix("host: ") for line in output("rustc", "-vV").splitlines()
                if line.startswith("host: "))
    packages = json.loads(output("cargo", "metadata", "--locked", "--format-version", "1",
                                 "--filter-platform", host))["packages"]
    unknown = compiled.difference(p["id"] for p in packages)
    if unknown:
        raise SystemExit(f"Cargo metadata omits compiled packages: {sorted(unknown)}")
    entries, texts = [], {}
    def notice_files(directory):
        return [p for p in directory.iterdir() if p.is_file() and
                p.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE"))]

    for pkg in sorted(packages, key=lambda p: p["id"]):
        if pkg["id"] not in compiled or pkg["source"] is None:
            continue
        root = Path(pkg["manifest_path"]).parent
        boundary = root
        if pkg["source"].startswith("git+"):
            boundary = Path(output("git", "-C", str(root), "rev-parse", "--show-toplevel"))
        files = notice_files(root)
        if pkg["license_file"]:
            files.append(root / pkg["license_file"])
        if not files and boundary != root:
            files = notice_files(boundary)
        entry = {key: pkg[key] for key in ("name", "version", "license", "authors", "repository", "source")}
        entry["files"] = {}
        for path in sorted(set(files)):
            if not path.resolve(strict=True).is_relative_to(boundary.resolve()):
                raise SystemExit(f"{pkg['id']} declares a notice outside its source directory.")
            content = path.read_bytes()
            digest = hashlib.sha256(content).hexdigest()
            texts[digest] = content
            entry["files"][path.name] = f"licenses/{digest}.txt"
        entries.append(entry)
    notices = {"THIRD-PARTY.json": (json.dumps(entries, sort_keys=True, indent=2) + "\n").encode()}
    notices.update({f"licenses/{key}.txt": value for key, value in texts.items()})
    return notices


def package(destination):
    if platform.system() != "Linux":
        raise SystemExit("Run the native package build on Linux.")
    if output("git", "status", "--porcelain"):
        raise SystemExit("Commit the release source before packaging it.")
    commit = output("git", "rev-parse", "HEAD")
    stamp = int(output("git", "show", "-s", "--format=%ct", commit))
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]
    name, version = manifest["name"], manifest["version"]
    messages = output("cargo", "build", "--locked", "--release", "--bin", name,
                      "--message-format=json-render-diagnostics")
    artifacts = [m for m in map(json.loads, messages.splitlines()) if m.get("reason") == "compiler-artifact"]
    binaries = [m["executable"] for m in artifacts if m.get("executable")
                and m["target"]["kind"] == ["bin"] and m["target"]["name"] == name]
    if len(binaries) != 1 or output(binaries[0], "--version") != f"{name} {version}":
        raise SystemExit("Cargo must produce one runnable release binary with the package version.")
    binary = Path(binaries[0])
    arch = platform.machine()
    # A Linux host can execute a foreign ELF through binfmt, so check its header.
    with binary.open("rb") as stream:
        elf = stream.read(20)
    with Path("/proc/self/exe").open("rb") as stream:
        host = stream.read(20)
    if elf[:4] != b"\x7fELF" or elf[4:6] != host[4:6] or elf[18:20] != host[18:20]:
        raise SystemExit("The release ELF architecture differs from the local Python runtime.")
    prefix = f"{name}-{version}-{commit[:12]}-linux-{arch}"
    release = destination / (prefix + ".tar.gz")
    checksum = release.with_name(release.name + ".sha256")
    if release.exists() or checksum.exists():
        raise SystemExit(f"Package already exists in {destination}")
    documents = ["LICENSE", "NOTICE", "Cargo.lock", "docs/linux-install.md", "docs/daemon.md",
                 "docs/adapters.md", "docs/configuration.md", "docs/memory-resources.md",
                 "docs/review.md", "docs/notifications.md", "docs/neutral-archive.md",
                 "docs/mempal-migration.md", "docs/licenses/mempal-MIT.txt",
                 "examples/config.toml", "examples/notifier-file.py", "src/config/defaults.toml"]
    documents.extend(output("git", "ls-tree", "-r", "--name-only", commit, "docs/adapters").splitlines())
    notices = dependency_notices({m["package_id"] for m in artifacts})
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=destination) as tmp:
        staged = Path(tmp) / release.name
        with staged.open("xb") as raw, gzip.GzipFile(filename="", mode="wb", fileobj=raw,
                mtime=0, compresslevel=6) as gz, tarfile.open(fileobj=gz, mode="w|") as archive:
            with binary.open("rb") as stream:
                add(archive, f"{prefix}/bin/{name}", stream, binary.stat().st_size, 0o755, stamp)
            for path in sorted(documents):
                content = subprocess.check_output(["git", "show", f"{commit}:{path}"], cwd=ROOT)
                mode = 0o755 if path == "examples/notifier-file.py" else 0o644
                add(archive, f"{prefix}/{path}", io.BytesIO(content), len(content), mode, stamp)
            for path, content in sorted(notices.items()):
                add(archive, f"{prefix}/{path}", io.BytesIO(content), len(content), 0o644, stamp)
            content = (commit + "\n").encode()
            add(archive, f"{prefix}/SOURCE_COMMIT", io.BytesIO(content), len(content), 0o644, stamp)
        if output("git", "rev-parse", "HEAD") != commit or output("git", "diff", "HEAD", "--name-only"):
            raise SystemExit("Source changed during the release build.")
        with staged.open("rb") as stream:
            line = f"{hashlib.file_digest(stream, 'sha256').hexdigest()}  {release.name}\n"
        sums = Path(tmp) / checksum.name
        sums.write_text(line)
        release.hardlink_to(staged)
        try:
            checksum.hardlink_to(sums)
        except OSError:
            release.unlink()
            raise
    print(line, end="")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path, help="directory for the archive and checksum")
    package(parser.parse_args().destination.resolve())
