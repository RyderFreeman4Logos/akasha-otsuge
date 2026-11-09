use akasha_otsuge::outcome::Workspace;
use rustix::fs::{Mode, OFlags, ResolveFlags};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    fs::{File, Metadata},
    io::{self, Read},
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::Path,
    process::{Command, Stdio},
};

pub(super) fn hex(bytes: &[u8]) -> String {
    akasha_otsuge::capture::encode_hex(bytes)
}

pub(super) fn hash(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn digest(mut input: impl Read) -> io::Result<(u64, String)> {
    let mut digest = Sha256::new();
    let mut bytes = 0;
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
        bytes += count as u64;
    }
    Ok((bytes, hex(&digest.finalize())))
}

fn identity(m: &Metadata) -> (u64, u64, u64, i64, i64, i64, i64, u32) {
    (
        m.dev(),
        m.ino(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
        m.mode(),
    )
}

pub(super) fn file(root: &File, path: &OsStr) -> io::Result<(u64, String, u32)> {
    let open = || {
        rustix::fs::openat2(
            root,
            path,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
            ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS,
        )
    };
    let input = File::from(open()?);
    let before = input.metadata()?;
    if !before.is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    let (bytes, hash) = digest((&input).take(before.len().saturating_add(1)))?;
    if bytes != before.len()
        || identity(&before) != identity(&input.metadata()?)
        || identity(&before) != identity(&File::from(open()?).metadata()?)
    {
        return Err(io::Error::other("file changed during observation"));
    }
    Ok((bytes, hash, before.mode() & 0o777))
}

fn git(path: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(path)
        .args(["--no-optional-locks", "-c", "core.fsmonitor=false"])
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped());
    // Git's inherited repository selectors must not redirect the bound workspace.
    for (key, _) in std::env::vars_os().filter(|(key, _)| key.as_bytes().starts_with(b"GIT_")) {
        command.env_remove(key);
    }
    command
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
    command
}

fn read_git(path: &Path, args: &[&str]) -> io::Result<Vec<u8>> {
    let mut child = git(path, args).spawn()?;
    let mut bytes = Vec::new();
    let result = child
        .stdout
        .take()
        .unwrap()
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes);
    if result.is_err() || bytes.len() > 4 * 1024 * 1024 {
        let _ = child.kill();
        let _ = child.wait();
        return Err(io::Error::other("git metadata unavailable or too large"));
    }
    if !child.wait()?.success() {
        return Err(io::Error::other("git failed"));
    }
    Ok(bytes)
}

fn diff(path: &Path) -> io::Result<String> {
    let mut child = git(
        path,
        &[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--binary",
            "HEAD",
            "--",
            ".",
        ],
    )
    .spawn()?;
    let result = digest(child.stdout.take().unwrap());
    if result.is_err() {
        let _ = child.kill();
    }
    if !child.wait()?.success() {
        return Err(io::Error::other("git diff failed"));
    }
    Ok(result?.1)
}

pub(super) fn observe(path: &Path, root: &File) -> Workspace {
    let mut snapshot = Workspace {
        base_commit: None,
        index_sha256: None,
        diff_sha256: None,
        tree_sha256: None,
        gaps: Vec::new(),
    };
    let base = read_git(path, &["rev-parse", "--verify", "HEAD"]).ok();
    let index = read_git(path, &["ls-files", "--stage", "-z", "--", "."]).ok();
    let list_args = [
        "ls-files",
        "--cached",
        "--others",
        "--exclude-standard",
        "-z",
        "--",
        ".",
    ];
    let list = read_git(path, &list_args).ok();
    snapshot.base_commit = base
        .as_deref()
        .and_then(|b| std::str::from_utf8(b).ok())
        .map(|s| s.trim().into());
    snapshot.index_sha256 = index.as_deref().map(hash);
    snapshot.diff_sha256 = diff(path).ok();
    if let Some(list) = &list {
        let mut names: Vec<_> = list.split(|b| *b == 0).filter(|b| !b.is_empty()).collect();
        names.sort_unstable();
        names.dedup();
        let mut tree = Sha256::new();
        let mut complete = true;
        for name in names {
            tree.update((name.len() as u64).to_le_bytes());
            tree.update(name);
            match file(root, OsStr::from_bytes(name)) {
                Ok((bytes, hash, mode)) => {
                    tree.update(b"file");
                    tree.update(mode.to_le_bytes());
                    tree.update(bytes.to_le_bytes());
                    tree.update(hash);
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => tree.update(b"absent"),
                Err(_) => {
                    complete = false;
                }
            }
        }
        if complete {
            snapshot.tree_sha256 = Some(hex(&tree.finalize()));
        }
    }
    for (present, gap) in [
        (snapshot.base_commit.is_some(), "git_base_unavailable"),
        (snapshot.index_sha256.is_some(), "git_index_unavailable"),
        (snapshot.diff_sha256.is_some(), "git_diff_unavailable"),
        (snapshot.tree_sha256.is_some(), "tree_incomplete"),
    ] {
        if !present {
            snapshot.gaps.push(gap.into());
        }
    }
    if base != read_git(path, &["rev-parse", "--verify", "HEAD"]).ok()
        || index != read_git(path, &["ls-files", "--stage", "-z", "--", "."]).ok()
        || list != read_git(path, &list_args).ok()
    {
        snapshot.gaps.push("git_changed_during_observation".into());
    }
    if !path
        .metadata()
        .and_then(|named| {
            root.metadata()
                .map(|pinned| named.dev() == pinned.dev() && named.ino() == pinned.ino())
        })
        .unwrap_or(false)
    {
        snapshot.gaps.push("workspace_replaced".into());
    }
    snapshot
}
