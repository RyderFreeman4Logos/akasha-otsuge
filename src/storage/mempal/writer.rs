use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::storage::archive::{ArchiveCommit, ArchiveManifest, BlobRevision, Sink};
use crate::storage::{
    Change, CommitReceipt, CommitRequest, RecordKey, RecordKind, Result, RevisionRef, Storage,
    StorageError, hash,
};

pub(super) struct Writer {
    commits: Sink,
    blobs: Sink,
    source: String,
    incremental: bool,
    pub observed_at_ms: u64,
    sequence: u64,
    revisions: u64,
    pending: Vec<Change>,
    pending_bytes: usize,
    completed_manifest: Option<Vec<u8>>,
    pub(super) projection: Option<ProjectionContext>,
    // Keep output locks until buffered sinks have been dropped.
    output: Output,
}

pub(super) struct ProjectionContext {
    pub store: Arc<Storage>,
    pub from: String,
    pub origin: String,
    pub generation: String,
    pub retained: std::collections::BTreeSet<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    mapping_version: u32,
    source_sha256: String,
    observed_at_ms: u64,
    uid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_archive_sha256: Option<String>,
}

fn conflict() -> StorageError {
    StorageError::new("MEMPAL_CONVERSION_CONFLICT")
}

fn unsafe_path() -> StorageError {
    StorageError::new("UNSAFE_STORAGE_PATH")
}

fn same_inode(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    (a.dev(), a.ino()) == (b.dev(), b.ino())
}

struct Output {
    path: PathBuf,
    parent_path: PathBuf,
    parent: File,
    directory: File,
    files: BTreeMap<String, File>,
    source_inode: (u64, u64),
}

impl Drop for Output {
    fn drop(&mut self) {
        // A concurrent fork can retain the open descriptions until its exec.
        if let Some(lock) = self.files.get(".mempal-convert.lock") {
            let _ = lock.unlock();
        }
        let _ = self.directory.unlock();
    }
}

impl Output {
    fn new(path: &Path, source: &super::reader::Snapshot, incremental: bool) -> Result<Self> {
        let parent_path = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let parent_path_canonical = parent_path.canonicalize()?;
        let name = path.file_name().ok_or_else(unsafe_path)?;
        if source.path.starts_with(parent_path_canonical.join(name)) {
            return Err(StorageError::new("UNSAFE_MEMPAL_SNAPSHOT"));
        }
        let parent = OpenOptions::new()
            .read(true)
            .custom_flags(
                (rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::NOFOLLOW).bits() as i32,
            )
            .open(&parent_path_canonical)?;
        match rustix::fs::mkdirat(
            &parent,
            name,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR | rustix::fs::Mode::XUSR,
        ) {
            Ok(()) | Err(rustix::io::Errno::EXIST) => (),
            Err(error) => return Err(std::io::Error::from(error).into()),
        }
        let directory = File::from(
            rustix::fs::openat(
                &parent,
                name,
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::DIRECTORY
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            )
            .map_err(std::io::Error::from)?,
        );
        let metadata = directory.metadata()?;
        if metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o777 != 0o700
        {
            return Err(unsafe_path());
        }
        // The directory lock survives renaming or replacing a lock filename.
        directory
            .try_lock()
            .map_err(|_| StorageError::new("MEMPAL_CONVERSION_BUSY"))?;
        let mut output = Self {
            path: path.into(),
            parent_path: parent_path.into(),
            parent,
            directory,
            files: BTreeMap::new(),
            source_inode: source.file_identity(),
        };
        output
            .file(".mempal-convert.lock", true)?
            .try_lock()
            .map_err(|_| StorageError::new("MEMPAL_CONVERSION_BUSY"))?;
        output.validate()?;
        for entry in fs::read_dir(output.directory_path())? {
            let name = entry?.file_name();
            let name = name.to_str().ok_or_else(conflict)?;
            if incremental && name == ".mempal-index" {
                crate::storage::layout::directory(&output.directory_path().join(name))?;
                continue;
            }
            if !matches!(
                name,
                ".mempal-convert.lock"
                    | "conversion.json"
                    | "conversion.json.tmp"
                    | "commits.jsonl"
                    | "blobs.bin"
                    | "manifest.json"
                    | "manifest.json.tmp"
            ) {
                return Err(conflict());
            }
            output.file(name, false)?;
        }
        Ok(output)
    }

    fn directory_path(&self) -> PathBuf {
        format!("/proc/self/fd/{}", self.directory.as_raw_fd()).into()
    }

    fn file(&mut self, name: &str, create: bool) -> Result<File> {
        let mut flags = rustix::fs::OFlags::RDWR
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC;
        if create {
            flags |= rustix::fs::OFlags::CREATE;
        }
        let file = File::from(
            rustix::fs::openat(
                &self.directory,
                name,
                flags,
                rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
            )
            .map_err(std::io::Error::from)?,
        );
        let metadata = file.metadata()?;
        if (metadata.dev(), metadata.ino()) == self.source_inode {
            return Err(StorageError::new("UNSAFE_MEMPAL_SNAPSHOT"));
        }
        if !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o777 != 0o600
        {
            return Err(unsafe_path());
        }
        if let Some(held) = self.files.get(name) {
            if !same_inode(&held.metadata()?, &metadata) {
                return Err(conflict());
            }
        } else {
            self.files.insert(name.into(), file.try_clone()?);
        }
        Ok(file)
    }

    fn small_file(&mut self, name: &str) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        self.file(name, false)?
            .take(64 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 64 * 1024 {
            return Err(conflict());
        }
        Ok(bytes)
    }

    fn present(&self, name: &str) -> Result<bool> {
        match fs::symlink_metadata(self.directory_path().join(name)) {
            Ok(metadata) if metadata.is_symlink() => Err(unsafe_path()),
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    fn validate(&self) -> Result<()> {
        let directory = self.directory.metadata()?;
        if !same_inode(&fs::metadata(&self.parent_path)?, &self.parent.metadata()?)
            || !same_inode(&fs::symlink_metadata(&self.path)?, &directory)
            || directory.uid() != rustix::process::geteuid().as_raw()
            || directory.mode() & 0o777 != 0o700
        {
            return Err(conflict());
        }
        for (name, held) in &self.files {
            let metadata = fs::symlink_metadata(self.directory_path().join(name))?;
            if !same_inode(&metadata, &held.metadata()?)
                || !metadata.is_file()
                || metadata.nlink() != 1
                || metadata.uid() != rustix::process::geteuid().as_raw()
                || metadata.mode() & 0o777 != 0o600
            {
                return Err(conflict());
            }
        }
        Ok(())
    }

    fn sink(&mut self, name: &str, complete: bool) -> Result<Sink> {
        let file = self.file(name, !complete)?;
        let prefix = self.file(name, false)?;
        Sink::resume(file, prefix, complete)
    }

    fn publish(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        self.validate()?;
        let file = publish(&self.directory, name, bytes)?;
        self.files.insert(name.into(), file);
        self.sync()
    }

    fn sync(&self) -> Result<()> {
        self.directory.sync_all()?;
        self.parent.sync_all()?;
        self.validate()
    }
}

pub(super) fn publish(directory: &File, name: &str, bytes: &[u8]) -> Result<File> {
    // Anonymous bytes cannot be replaced through a staging pathname before publication.
    let mut file = File::from(
        rustix::fs::openat(
            directory,
            ".",
            rustix::fs::OFlags::TMPFILE | rustix::fs::OFlags::RDWR | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        )
        .map_err(std::io::Error::from)?,
    );
    file.write_all(bytes)?;
    file.sync_all()?;
    rustix::fs::linkat(
        rustix::fs::CWD,
        format!("/proc/self/fd/{}", file.as_raw_fd()),
        directory,
        name,
        rustix::fs::AtFlags::SYMLINK_FOLLOW,
    )
    .map_err(|error| {
        if error == rustix::io::Errno::EXIST {
            conflict()
        } else {
            std::io::Error::from(error).into()
        }
    })?;
    #[cfg(test)]
    if name == "manifest.json" {
        crate::storage::crash_tests::checkpoint("mempal-manifest-renamed");
    }
    directory.sync_all()?;
    Ok(file)
}

impl Writer {
    pub fn group(&mut self, build: impl Fn(u64) -> Result<Vec<Change>>) -> Result<()> {
        let next = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| StorageError::new("INVALID_STORAGE_REQUEST"))?;
        let mut changes = build(next)?;
        let mut bytes = changes
            .iter()
            .map(|change| change.content.len())
            .sum::<usize>();
        if self.pending.len() + changes.len() > 64 || self.pending_bytes + bytes > 1024 * 1024 {
            self.flush()?;
            changes = build(
                self.sequence
                    .checked_add(1)
                    .ok_or_else(|| StorageError::new("INVALID_STORAGE_REQUEST"))?,
            )?;
            bytes = changes.iter().map(|change| change.content.len()).sum();
        }
        if changes.len() > 64 || bytes > 1024 * 1024 {
            return Err(StorageError::new("INVALID_STORAGE_REQUEST"));
        }
        self.pending_bytes += bytes;
        self.pending.extend(changes);
        Ok(())
    }

    pub fn new(
        destination: &Path,
        source: &super::reader::Snapshot,
        observed_at_ms: u64,
    ) -> Result<Self> {
        Self::open(destination, source, observed_at_ms, None)
    }

    pub(super) fn open(
        destination: &Path,
        source: &super::reader::Snapshot,
        observed_at_ms: u64,
        previous_archive_sha256: Option<String>,
    ) -> Result<Self> {
        let mapping_version = if previous_archive_sha256.is_some() {
            3
        } else {
            super::MAPPING_VERSION
        };
        let mut output = Output::new(destination, source, previous_archive_sha256.is_some())?;
        let source = source.sha256.as_str();
        let identity = if output.present("conversion.json")? {
            let identity: Identity = serde_json::from_slice(&output.small_file("conversion.json")?)
                .map_err(|_| conflict())?;
            if identity.mapping_version != mapping_version
                || identity.source_sha256 != source
                || identity.previous_archive_sha256 != previous_archive_sha256
                || identity.uid != rustix::process::geteuid().as_raw()
                || identity.observed_at_ms > i64::MAX as u64
            {
                return Err(conflict());
            }
            identity
        } else {
            for name in [
                "blobs.bin",
                "commits.jsonl",
                "manifest.json",
                "manifest.json.tmp",
            ] {
                if output.present(name)? {
                    return Err(conflict());
                }
            }
            let identity = Identity {
                mapping_version,
                source_sha256: source.into(),
                observed_at_ms,
                uid: rustix::process::geteuid().as_raw(),
                previous_archive_sha256,
            };
            output.publish("conversion.json", &serde_json::to_vec(&identity)?)?;
            identity
        };
        let completed_manifest = output
            .present("manifest.json")?
            .then(|| output.small_file("manifest.json"))
            .transpose()?;
        #[cfg(test)]
        crate::storage::crash_tests::checkpoint("mempal-identity");
        let commits = output.sink("commits.jsonl", completed_manifest.is_some())?;
        let blobs = output.sink("blobs.bin", completed_manifest.is_some())?;
        output.validate()?;
        Ok(Self {
            commits,
            blobs,
            output,
            source: source.into(),
            incremental: mapping_version == 3,
            observed_at_ms: identity.observed_at_ms,
            sequence: 0,
            revisions: 0,
            pending: Vec::new(),
            pending_bytes: 0,
            completed_manifest,
            projection: None,
        })
    }

    pub(super) fn index_path(&self) -> PathBuf {
        self.output.directory_path().join(".mempal-index")
    }

    pub(super) fn seed(&mut self, store: &Storage, generation: &str) -> Result<()> {
        (self.sequence, self.revisions) =
            store.write_archive_sinks(&mut self.commits, &mut self.blobs)?;
        self.source = generation.into();
        Ok(())
    }

    pub fn push(&mut self, id: String, value: &impl serde::Serialize) -> Result<()> {
        let content = serde_json::to_string(value)?;
        if self.pending.len() == 64 || self.pending_bytes + content.len() > 1024 * 1024 {
            self.flush()?;
        }
        self.pending_bytes += content.len();
        self.pending.push(Change {
            key: RecordKey {
                kind: RecordKind::Evidence,
                id,
            },
            expected_revision: None,
            content,
        });
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| StorageError::new("INVALID_STORAGE_REQUEST"))?;
        let mut request = CommitRequest {
            request_id: if self.incremental {
                // Neutral archives are ordered by request ID, including every earlier generation.
                format!("mempal/z/{:020}/{}", self.sequence, self.source)
            } else {
                format!("mempal/{}/commit/{:020}", self.source, self.sequence)
            },
            config_generation: 1,
            policy_revision: Some("mempal-unverified-v1".into()),
            changes: std::mem::take(&mut self.pending),
        };
        request.validate()?;
        request.changes.sort_by(|a, b| a.key.cmp(&b.key));
        let request_sha256 = hash(&serde_json::to_vec(&request)?);
        let mut revisions = Vec::with_capacity(request.changes.len());
        for change in &request.changes {
            let offset = self.blobs.bytes;
            self.blobs.write(change.content.as_bytes())?;
            revisions.push(BlobRevision {
                key: change.key.clone(),
                commit_seq: self.sequence,
                previous: change.expected_revision,
                content_sha256: hash(change.content.as_bytes()),
                blob_offset: offset,
                blob_bytes: change.content.len() as u64,
            });
        }
        self.revisions += revisions.len() as u64;
        #[cfg(test)]
        crate::storage::crash_tests::checkpoint("mempal-partial");
        let receipt = CommitReceipt {
            request_id: request.request_id,
            request_sha256,
            commit_seq: self.sequence,
            config_generation: request.config_generation,
            policy_revision: request.policy_revision,
            revisions: revisions
                .iter()
                .map(|revision| RevisionRef {
                    key: revision.key.clone(),
                    commit_seq: revision.commit_seq,
                })
                .collect(),
        };
        let entry = ArchiveCommit {
            receipt,
            recorded_at_ms: self.observed_at_ms,
            revisions,
        };
        let mut bytes = serde_json::to_vec(&entry)?;
        bytes.push(b'\n');
        self.commits.write(&bytes)?;
        self.pending_bytes = 0;
        Ok(())
    }

    pub fn finish(mut self) -> Result<ArchiveManifest> {
        self.flush()?;
        let manifest = ArchiveManifest {
            archive_version: 1,
            record_format_version: u32::from(crate::storage::FORMAT_VERSION),
            commit_seq: self.sequence,
            commits: self.sequence,
            revisions: self.revisions,
            files: BTreeMap::from([
                ("blobs.bin".into(), self.blobs.finish()?),
                ("commits.jsonl".into(), self.commits.finish()?),
            ]),
        };
        let bytes = serde_json::to_vec(&manifest)?;
        #[cfg(test)]
        crate::storage::crash_tests::checkpoint("mempal-before-publish");
        match self.completed_manifest {
            Some(stored) if stored != bytes => return Err(conflict()),
            Some(_) => self.output.sync()?,
            None => self.output.publish("manifest.json", &bytes)?,
        }
        #[cfg(test)]
        crate::storage::crash_tests::checkpoint("mempal-published");
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::layout;

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn writer_release_does_not_wait_for_inherited_descriptor_copies() {
        let mut blocked = Vec::new();
        for member in ["directory", ".mempal-convert.lock"] {
            let root = std::env::temp_dir().join(format!(
                "akasha-mempal-release-{}-{member}",
                std::process::id()
            ));
            fs::create_dir(&root).unwrap();
            let _fixture = Fixture(root.clone());
            let source_path = root.join("source.db");
            let db = rusqlite::Connection::open(&source_path).unwrap();
            db.execute_batch(include_str!("../../../tests/fixtures/mempal21-28.sql"))
                .unwrap();
            db.close().unwrap();
            let source = super::super::reader::Snapshot::open(&source_path).unwrap();
            let output = root.join("archive");
            let writer = Writer::new(&output, &source, 1).unwrap();
            let retained = if member == "directory" {
                writer.output.directory.try_clone().unwrap()
            } else {
                writer.output.files[member].try_clone().unwrap()
            };
            assert_eq!(
                Writer::new(&output, &source, 1)
                    .err()
                    .map(|error| error.code),
                Some("MEMPAL_CONVERSION_BUSY")
            );
            drop(writer);
            let next = Writer::new(&output, &source, 1);
            if let Err(error) = &next {
                blocked.push((member, error.code));
            }
            drop(next);
            drop(retained);
        }
        assert!(
            blocked.is_empty(),
            "released output still excludes successors with descriptor copies: {blocked:?}"
        );
    }

    #[test]
    fn metadata_publication_preserves_unexpected_final_occupants() {
        let mut damaged = Vec::new();
        for name in ["conversion.json", "manifest.json"] {
            let root = std::env::temp_dir().join(format!(
                "akasha-mempal-publish-{}-{name}",
                std::process::id()
            ));
            fs::create_dir(&root).unwrap();
            let _fixture = Fixture(root.clone());
            let source = root.join("source.db");
            let db = rusqlite::Connection::open(&source).unwrap();
            db.execute_batch(include_str!("../../../tests/fixtures/mempal21-28.sql"))
                .unwrap();
            db.close().unwrap();
            let pinned = File::open(&source).unwrap();
            let identity = pinned.metadata().unwrap();
            let bytes = fs::read(&source).unwrap();
            let output = root.join("archive");
            layout::private_dir(&output, false).unwrap();
            let moved = output.join(name);
            fs::rename(&source, &moved).unwrap();
            let result = publish(&File::open(&output).unwrap(), name, b"new metadata");
            let preserved = fs::read(&moved).ok().as_deref() == Some(bytes.as_slice())
                && fs::metadata(&moved)
                    .is_ok_and(|m| (m.dev(), m.ino()) == (identity.dev(), identity.ino()));
            eprintln!(
                "publication={name} refused={} moved_source_preserved={preserved}",
                result.is_err()
            );
            if result.is_ok() || !preserved {
                damaged.push(name);
            }
        }
        assert!(
            damaged.is_empty(),
            "publication replaced source occupants: {damaged:?}"
        );
    }

    #[test]
    fn writer_rejects_replaced_directories_and_members() {
        use std::os::unix::fs::PermissionsExt;
        let mut missed = Vec::new();
        for name in [
            "directory",
            "parent",
            ".mempal-convert.lock",
            "conversion.json",
            "commits.jsonl",
            "blobs.bin",
            "manifest.json",
        ] {
            let root = std::env::temp_dir()
                .join(format!("akasha-mempal-owner-{}-{name}", std::process::id()));
            fs::create_dir(&root).unwrap();
            let _fixture = Fixture(root.clone());
            let source_path = root.join("source.db");
            let db = rusqlite::Connection::open(&source_path).unwrap();
            db.execute_batch(include_str!("../../../tests/fixtures/mempal21-28.sql"))
                .unwrap();
            db.close().unwrap();
            fs::set_permissions(&source_path, fs::Permissions::from_mode(0o600)).unwrap();
            let before = fs::read(&source_path).unwrap();
            let source = super::super::reader::Snapshot::open(&source_path).unwrap();
            let parent = root.join("parent");
            layout::private_dir(&parent, false).unwrap();
            let output = parent.join("archive");
            if name == "manifest.json" {
                let mut initial = Writer::new(&output, &source, 1).unwrap();
                initial.push("row".into(), &"retained value").unwrap();
                initial.finish().unwrap();
            }
            let mut writer = Writer::new(&output, &source, 1).unwrap();
            writer.push("row".into(), &"retained value").unwrap();
            let mut foreign_lock = None;
            if name == "directory" {
                fs::rename(&output, root.join("parked")).unwrap();
                layout::private_dir(&output, false).unwrap();
                let lock = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(output.join(".mempal-convert.lock"))
                    .unwrap();
                lock.try_lock().unwrap();
                foreign_lock = Some(lock);
            } else if name == "parent" {
                fs::rename(&parent, root.join("parked")).unwrap();
                layout::private_dir(&parent, false).unwrap();
                fs::rename(root.join("parked/archive"), &output).unwrap();
            } else {
                fs::rename(output.join(name), root.join("parked-member")).unwrap();
                layout::write_new(&output.join(name), b"foreign occupant").unwrap();
                if name == ".mempal-convert.lock" && Writer::new(&output, &source, 1).is_ok() {
                    missed.push("second writer after lock replacement");
                }
            }
            let refused = writer.finish().is_err();
            let foreign_unchanged = if name == "directory" {
                let members: Vec<_> = fs::read_dir(&output)
                    .unwrap()
                    .map(|e| e.unwrap().file_name())
                    .collect();
                members == [std::ffi::OsString::from(".mempal-convert.lock")]
            } else if name == "parent" {
                true
            } else {
                fs::read(output.join(name)).unwrap() == b"foreign occupant"
            };
            eprintln!("replacement={name} refused={refused} foreign_unchanged={foreign_unchanged}");
            if !refused || !foreign_unchanged {
                missed.push(name);
            }
            assert_eq!(fs::read(&source_path).unwrap(), before);
            drop(foreign_lock);
        }
        assert!(
            missed.is_empty(),
            "writer lost output ownership: {missed:?}"
        );
    }
}
