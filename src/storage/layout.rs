use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use super::{Result, StorageError};

#[cfg(test)]
thread_local! {
    static BEFORE_ROOT_CREATE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

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

pub(super) fn open_root(admission: &Admission) -> Result<(super::import_publish::Root, bool)> {
    let root = admission.root().join("authority");
    let created = match fs::symlink_metadata(&root) {
        Ok(_) => {
            directory(&root)?;
            false
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            #[cfg(test)]
            BEFORE_ROOT_CREATE.with(|hook| {
                if let Some(hook) = hook.take() {
                    hook();
                }
            });
            private_dir(&root, false)?;
            admission.sync()?;
            true
        }
        Err(error) => return Err(error.into()),
    };
    let root = super::import_publish::Root::open(admission, "authority")?;
    root.validate(admission, "authority")?;
    if created {
        if fs::read_dir(root.path())?.next().is_some() {
            return Err(StorageError::new("IMPORT_CONFLICT"));
        }
    } else {
        check_format(&root.path())?;
        directory(&root.path().join("kv"))?;
    }
    Ok((root, created))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ordinary_open_does_not_initialize_a_replaced_data_directory() {
        struct Fixture(PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                fs::remove_dir_all(&self.0).unwrap();
            }
        }
        let base = std::env::temp_dir().join(format!("akasha-open-root-{}", std::process::id()));
        private_dir(&base, false).unwrap();
        let _fixture = Fixture(base.clone());
        let data = base.join("data");
        let foreign = base.join("foreign");
        let parked = base.join("parked");
        for path in [&data, &foreign] {
            private_dir(path, false).unwrap();
        }
        fs::write(foreign.join("keep"), b"foreign data").unwrap();
        let config = crate::config::resolve(
            "",
            None,
            "",
            &crate::config::Paths {
                data_dir: data.clone(),
                socket_path: base.join("daemon.sock"),
            },
        )
        .unwrap();
        let moved_data = data.clone();
        BEFORE_ROOT_CREATE.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                fs::rename(&moved_data, parked).unwrap();
                fs::rename(foreign, moved_data).unwrap();
            }));
        });
        let error = match crate::storage::Storage::open(&config).await {
            Ok(store) => {
                store.close().await.unwrap();
                None
            }
            Err(error) => Some(error.code),
        };
        let untouched = !data.join("authority").exists();
        eprintln!("ordinary_open error={error:?} foreign_untouched={untouched}");
        assert!(
            untouched,
            "ordinary open initialized storage in a foreign root"
        );
        assert_eq!(error, Some("IMPORT_CONFLICT"));
        assert_eq!(fs::read(data.join("keep")).unwrap(), b"foreign data");
        BEFORE_ROOT_CREATE.with(|hook| assert!(hook.borrow().is_none()));
    }
}
