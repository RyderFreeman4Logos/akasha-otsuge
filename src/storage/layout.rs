use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use super::{Result, StorageError};

pub(super) const FORMAT: &[u8] = b"{\"application\":\"akasha-otsuge\",\"schema_version\":2,\"engine\":\"surrealkv-1.0.0\",\"engine_revision\":\"4338a790c8edc56ef5ea1e46157427af4815be3c\"}\n";

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
