use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use super::{Result, StorageError};

pub(super) const FORMAT: &[u8] = b"{\"application\":\"akasha-otsuge\",\"schema_version\":2,\"engine\":\"surrealkv-1.0.0\",\"engine_revision\":\"4338a790c8edc56ef5ea1e46157427af4815be3c\"}\n";

pub(super) struct Admission {
    pub(super) directory: File,
    path: PathBuf,
    lock: File,
}

impl Admission {
    pub(super) fn acquire(path: &Path, exclusive: bool) -> Result<Self> {
        private_dir(path, true)?;
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(
                (rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::NOFOLLOW).bits() as i32,
            )
            .open(path)?;
        let acquire = |file: &File| {
            if exclusive {
                file.try_lock()
            } else {
                file.try_lock_shared()
            }
            .map_err(|_| StorageError::new("IMPORT_BUSY"))
        };
        // The directory lock still excludes owners after the compatible lock name is moved.
        acquire(&directory)?;
        let lock = File::from(
            rustix::fs::openat(
                &directory,
                ".archive-import.lock",
                rustix::fs::OFlags::RDWR
                    | rustix::fs::OFlags::CREATE
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::NONBLOCK
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
            )
            .map_err(std::io::Error::from)?,
        );
        let metadata = lock.metadata()?;
        if !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o777 != 0o600
        {
            return Err(StorageError::new("UNSAFE_STORAGE_PATH"));
        }
        acquire(&lock)?;
        let admission = Self {
            directory,
            path: path.to_path_buf(),
            lock,
        };
        admission.validate()?;
        Ok(admission)
    }

    pub(super) fn root(&self) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}/.", self.directory.as_raw_fd()))
    }

    pub(super) fn validate(&self) -> Result<()> {
        for (path, file) in [
            (self.path.clone(), &self.directory),
            (self.root().join(".archive-import.lock"), &self.lock),
        ] {
            let named = fs::symlink_metadata(path)?;
            let owned = file.metadata()?;
            if (named.dev(), named.ino()) != (owned.dev(), owned.ino()) {
                return Err(StorageError::new("IMPORT_CONFLICT"));
            }
        }
        Ok(())
    }

    pub(super) fn sync(&self) -> Result<()> {
        self.validate()?;
        self.directory.sync_all()?;
        self.validate()
    }
}

pub(super) fn private_dir(path: &Path, recursive: bool) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(recursive);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    Ok(builder.create(path)?)
}

pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

pub(super) fn directory(path: &Path) -> Result<()> {
    if !fs::symlink_metadata(path)?.file_type().is_dir() {
        return Err(StorageError::new("UNSAFE_STORAGE_PATH"));
    }
    Ok(())
}

pub(super) fn sync_dir(path: &Path) -> Result<()> {
    Ok(File::open(path)?.sync_all()?)
}

pub(super) fn check_format(root: &Path) -> Result<()> {
    let path = root.join("format.json");
    let invalid = || StorageError::new("UNSUPPORTED_STORAGE_FORMAT");
    let metadata = fs::symlink_metadata(&path).map_err(|_| invalid())?;
    if !metadata.file_type().is_file() || metadata.len() != FORMAT.len() as u64 {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(FORMAT.len() as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes != FORMAT {
        return Err(invalid());
    }
    Ok(())
}

pub(super) fn open_root(data_dir: &Path) -> Result<(PathBuf, bool)> {
    private_dir(data_dir, true)?;
    let root = data_dir.join("authority");
    let created = match fs::symlink_metadata(&root) {
        Ok(_) => {
            directory(&root)?;
            false
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            private_dir(&root, false)?;
            sync_dir(data_dir)?;
            true
        }
        Err(error) => return Err(error.into()),
    };
    if !created {
        check_format(&root)?;
        directory(&root.join("kv"))?;
    }
    Ok((root, created))
}
