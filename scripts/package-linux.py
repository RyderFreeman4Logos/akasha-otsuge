#!/usr/bin/env python3
"""Build a native Linux package from this checkout and print its SHA-256."""
import argparse
import gzip
import hashlib
import io
import itertools
import json
from pathlib import Path
import platform
import posixpath
import re
import subprocess
import tarfile
import tempfile
import tomllib
from urllib.parse import quote, unquote, urlsplit


ROOT = Path(__file__).resolve().parents[1]


def output(*command):
    return subprocess.check_output(command, cwd=ROOT, text=True).strip()


def add(archive, name, stream, size, mode, timestamp):
    header = tarfile.TarInfo(name)
    header.size = size
    header.mode = mode
    header.mtime = timestamp
    archive.addfile(header, stream)


# Guides keep repository-only reference definitions in a final paragraph. Their
# Markdown links also work when reading the source checkout without a package.
SOURCE_LINKS = "<!-- Source links for the Linux archive -->"
SOURCE_REFERENCE = re.compile(r"(\[source-[a-z0-9-]+\]: <)([^<>\n]+)(>\n?)")


def package_markdown(text, filename, members, paths, repository, commit):
    trimmed = text.rstrip(" \t\r\n")
    body, separator, footer = trimmed.rpartition("\n\n")
    if not footer.startswith(SOURCE_LINKS + "\n"):
        return text
    lines = footer.splitlines(keepends=True)
    for i in range(1, len(lines)):
        definition = SOURCE_REFERENCE.fullmatch(lines[i])
        if not definition:
            raise ValueError(f"{filename}: invalid source link definition {lines[i]!r}")
        prefix, destination, suffix = definition.groups()
        url = urlsplit(destination)
        if not url.path or url.scheme or url.netloc or url.path.startswith("/"):
            raise ValueError(f"{filename}: expected a relative repository link: {destination}")
        target = posixpath.normpath(posixpath.join(posixpath.dirname(filename), unquote(url.path)))
        if target in members or any(name.startswith(target + "/") for name in members):
            continue
        if target not in paths:
            raise ValueError(f"{filename}: missing repository link {destination}")
        link = f"{repository}/{paths[target]}/{commit}/{quote(target)}"
        if url.query:
            link += "?" + url.query
        if url.fragment:
            link += "#" + url.fragment
        lines[i] = prefix + link + suffix
    return body + separator + "".join(lines) + text[len(trimmed):]


def dependency_notices(compiled, native_builds):
    host = next(line.removeprefix("host: ") for line in output("rustc", "-vV").splitlines()
                if line.startswith("host: "))
    packages = json.loads(output("cargo", "metadata", "--locked", "--format-version", "1",
                                 "--filter-platform", host))["packages"]
    unknown = compiled.difference(p["id"] for p in packages)
    if unknown:
        raise SystemExit(f"Cargo metadata omits compiled packages: {sorted(unknown)}")
    entries, texts = [], {}
    saved = ROOT / "docs/licenses/upstream"
    sources = json.loads((saved / "sources.json").read_text())
    native = json.loads((saved / "native-components.json").read_text())

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
        if pkg["id"] in native_builds:
            if pkg["name"] not in native:
                raise SystemExit(f"Review the bundled native sources in {pkg['id']}")
            entry["build_static_archives"] = native_builds[pkg["id"]]
        named = {path.name: path for path in files}
        if not named:
            vcs = root / ".cargo_vcs_info.json"
            revision = json.loads(vcs.read_text())["git"]["sha1"] if vcs.is_file() else None
            if revision not in sources:
                raise SystemExit(f"Missing license text for {pkg['id']} at {revision}")
            entry["upstream_notice_url"] = sources[revision]["upstream"]
            boundary = saved
            named = {name: saved / filename for name, filename in sources[revision]["files"].items()}
        content_by_name = {}
        for name, path in sorted(named.items()):
            if not path.resolve(strict=True).is_relative_to(boundary.resolve()):
                raise SystemExit(f"{pkg['id']} declares a notice outside its source directory.")
            content_by_name[name] = path.read_bytes()
        if pkg["name"] in native:
            component = native[pkg["name"]]
            if pkg["version"] != component["version"]:
                raise SystemExit(f"Recheck the native notices for {pkg['id']}")
            for name, lines in component["notices"].items():
                path = (root / name).resolve(strict=True)
                if not path.is_relative_to(root.resolve()):
                    raise SystemExit(f"Native notice outside {root}: {name}")
                if lines:
                    start, end = lines
                    with path.open("rb") as f:
                        content = b"".join(itertools.islice(f, start - 1, end))
                    if b"opyright" not in content:
                        raise SystemExit(f"Missing copyright declaration in {path}")
                    content_by_name[f"{name}#L{start}-L{end}"] = content
                else:
                    content_by_name[name] = path.read_bytes()
        entry["files"] = {}
        for name, content in sorted(content_by_name.items()):
            digest = hashlib.sha256(content).hexdigest()
            texts[digest] = content
            entry["files"][name] = f"licenses/{digest}.txt"
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
    messages = list(map(json.loads, messages.splitlines()))
    artifacts = [m for m in messages if m.get("reason") == "compiler-artifact"]
    native_builds = {}
    for message in messages:
        if message.get("reason") == "build-script-executed":
            archives = sorted(p.name for p in Path(message["out_dir"]).glob("*.a"))
            if archives:
                native_builds[message["package_id"]] = archives
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
    documents = ["LICENSE", "NOTICE", "Cargo.lock", "docs/licenses/mempal-MIT.txt",
                 "examples/config.toml", "examples/notifier-file.py", "scripts/linux-upgrade.py",
                 "src/config/defaults.toml"]
    documents.extend(f"docs/{p}" for p in output("git", "ls-tree", "--name-only", f"{commit}:docs").splitlines()
                     if p.endswith(".md"))
    documents.extend(p for p in output("git", "ls-tree", "-r", "--name-only", commit, "docs/adapters").splitlines()
                     if p.endswith("/README.md"))
    notices = dependency_notices({m["package_id"] for m in artifacts}, native_builds)
    members = set(documents) | set(notices) | {"SOURCE_COMMIT", f"bin/{name}"}
    tree = subprocess.check_output(["git", "ls-tree", "-rtz", commit], cwd=ROOT)
    paths = {}
    for entry in tree.decode().split("\0"):
        if entry:
            metadata, path = entry.split("\t", 1)
            paths[path] = metadata.split()[1]
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=destination) as tmp:
        staged = Path(tmp) / release.name
        with staged.open("xb") as raw, gzip.GzipFile(filename="", mode="wb", fileobj=raw,
                mtime=0, compresslevel=6) as gz, tarfile.open(fileobj=gz, mode="w|") as archive:
            with binary.open("rb") as stream:
                add(archive, f"{prefix}/bin/{name}", stream, binary.stat().st_size, 0o755, stamp)
            for path in sorted(documents):
                content = subprocess.check_output(["git", "show", f"{commit}:{path}"], cwd=ROOT)
                if path.endswith(".md"):
                    content = package_markdown(content.decode(), path, members, paths,
                                               manifest["repository"], commit).encode()
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
