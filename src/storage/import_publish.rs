use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::import::ImportIdentity;
use super::{Result, StorageError, layout};

const MARKER: &str = ".archive-replace.json";

fn conflict() -> StorageError {
    StorageError::new("IMPORT_CONFLICT")
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct DirectoryId {
    device: u64,
    inode: u64,
}

impl DirectoryId {
    fn metadata(metadata: &fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }

    fn matches(&self, path: &Path) -> Result<bool> {
        match fs::symlink_metadata(path) {
            Ok(m) => Ok(m.is_dir() && *self == Self::metadata(&m)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
}

pub(super) struct Root {
    file: File,
    id: DirectoryId,
}

impl Root {
    pub(super) fn open(admission: &layout::Admission, name: &str) -> Result<Self> {
        let file = File::from(
            rustix::fs::openat(
                &admission.directory,
                name,
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::DIRECTORY
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            )
            .map_err(std::io::Error::from)?,
        );
        let metadata = file.metadata()?;
        if metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o777 != 0o700
        {
            return Err(StorageError::new("UNSAFE_STORAGE_PATH"));
        }
        Ok(Self {
            id: DirectoryId::metadata(&metadata),
            file,
        })
    }

    pub(super) fn path(&self) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}/.", self.file.as_raw_fd()))
    }

    pub(super) fn validate(&self, admission: &layout::Admission, name: &str) -> Result<()> {
        admission.validate()?;
        if !self.id.matches(&admission.root().join(name))? {
            return Err(conflict());
        }
        Ok(())
    }

    pub(super) fn remove(&self, admission: &layout::Admission, name: &str) -> Result<()> {
        self.validate(admission, name)?;
        // Walk the retained inode, never a replacement of its staging name.
        for entry in fs::read_dir(self.path())? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                fs::remove_dir_all(entry.path())?;
            } else {
                fs::remove_file(entry.path())?;
            }
        }
        self.validate(admission, name)?;
        rustix::fs::unlinkat(&admission.directory, name, rustix::fs::AtFlags::REMOVEDIR)
            .map_err(std::io::Error::from)?;
        Ok(())
    }
}

#[derive(Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    directory: DirectoryId,
    identity: ImportIdentity,
}

#[derive(Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Replacement {
    old: Snapshot,
    new: Snapshot,
}

impl Replacement {
    pub(super) fn new(
        old: &Root,
        old_identity: ImportIdentity,
        new: &Root,
        new_identity: ImportIdentity,
    ) -> Self {
        Self {
            old: Snapshot {
                directory: old.id.clone(),
                identity: old_identity,
            },
            new: Snapshot {
                directory: new.id.clone(),
                identity: new_identity,
            },
        }
    }

    pub(super) fn load(admission: &layout::Admission) -> Result<Option<Self>> {
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
            )
            .open(admission.root().join(MARKER))
        {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.len() > 2048
            || metadata.nlink() != 1
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o777 != 0o600
        {
            return Err(conflict());
        }
        let mut bytes = Vec::new();
        file.take(2049).read_to_end(&mut bytes)?;
        if bytes.len() > 2048 {
            return Err(conflict());
        }
        let marker: Self = serde_json::from_slice(&bytes).map_err(|_| conflict())?;
        if marker.old.directory == marker.new.directory
            || marker.new.identity.commit_seq <= marker.old.identity.commit_seq
            || !marker.old.identity.valid()
            || !marker.new.identity.valid()
        {
            return Err(conflict());
        }
        Ok(Some(marker))
    }

    pub(super) fn published(
        &self,
        admission: &layout::Admission,
        input: &ImportIdentity,
    ) -> Result<bool> {
        if input != &self.new.identity {
            return Err(conflict());
        }
        if self
            .new
            .directory
            .matches(&admission.root().join("authority"))?
        {
            return Ok(true);
        }
        if !self
            .old
            .directory
            .matches(&admission.root().join("authority"))?
            || !self
                .new
                .directory
                .matches(&admission.root().join(".archive-import"))?
        {
            return Err(conflict());
        }
        Ok(false)
    }

    pub(super) fn validate_identity(
        &self,
        identity: &ImportIdentity,
        published: bool,
    ) -> Result<()> {
        let expected = if published {
            &self.new.identity
        } else {
            &self.old.identity
        };
        if identity != expected {
            return Err(conflict());
        }
        Ok(())
    }

    pub(super) fn publish(&self, admission: &layout::Admission) -> Result<()> {
        admission.validate()?;
        if !self
            .old
            .directory
            .matches(&admission.root().join("authority"))?
            || !self
                .new
                .directory
                .matches(&admission.root().join(".archive-import"))?
        {
            return Err(conflict());
        }
        match Self::load(admission)? {
            Some(stored) if stored == *self => (),
            Some(_) => return Err(conflict()),
            None => {
                let mut file = File::from(
                    rustix::fs::openat(
                        &admission.directory,
                        ".",
                        rustix::fs::OFlags::TMPFILE
                            | rustix::fs::OFlags::RDWR
                            | rustix::fs::OFlags::CLOEXEC,
                        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
                    )
                    .map_err(std::io::Error::from)?,
                );
                file.write_all(&serde_json::to_vec(self)?)?;
                file.sync_all()?;
                rustix::fs::linkat(
                    rustix::fs::CWD,
                    format!("/proc/self/fd/{}", file.as_raw_fd()),
                    &admission.directory,
                    MARKER,
                    rustix::fs::AtFlags::SYMLINK_FOLLOW,
                )
                .map_err(std::io::Error::from)?;
                admission.sync()?;
            }
        }
        #[cfg(test)]
        super::crash_tests::checkpoint("incremental-marker");
        admission.validate()?;
        if !self
            .old
            .directory
            .matches(&admission.root().join("authority"))?
            || !self
                .new
                .directory
                .matches(&admission.root().join(".archive-import"))?
            || Self::load(admission)?.as_ref() != Some(self)
        {
            return Err(conflict());
        }
        rustix::fs::renameat_with(
            &admission.directory,
            ".archive-import",
            &admission.directory,
            "authority",
            rustix::fs::RenameFlags::EXCHANGE,
        )
        .map_err(std::io::Error::from)?;
        #[cfg(test)]
        super::crash_tests::checkpoint("incremental-exchanged");
        self.cleanup(admission)
    }

    pub(super) fn cleanup(&self, admission: &layout::Admission) -> Result<()> {
        admission.sync()?;
        if !self
            .new
            .directory
            .matches(&admission.root().join("authority"))?
            || Self::load(admission)?.as_ref() != Some(self)
        {
            return Err(conflict());
        }
        #[cfg(test)]
        super::crash_tests::checkpoint("incremental-synced");
        let stage = admission.root().join(".archive-import");
        match fs::symlink_metadata(&stage) {
            Ok(_) => {
                if !self.old.directory.matches(&stage)? {
                    return Err(conflict());
                }
                Root::open(admission, ".archive-import")?.remove(admission, ".archive-import")?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
        admission.sync()?;
        #[cfg(test)]
        super::crash_tests::checkpoint("incremental-cleaned");
        if Self::load(admission)?.as_ref() != Some(self) {
            return Err(conflict());
        }
        rustix::fs::unlinkat(&admission.directory, MARKER, rustix::fs::AtFlags::empty())
            .map_err(std::io::Error::from)?;
        admission.sync()
    }
}
