use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use super::{Result, Storage, StorageError, layout};
use crate::config::ResolvedConfig;

const MAX_MANIFEST_BYTES: u64 = 32 * 1024 * 1024;
const MAX_FILES: usize = 100_000;

#[derive(Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct BackupFile {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    commit_seq: u64,
    files: Vec<BackupFile>,
}

fn invalid() -> StorageError {
    StorageError::new("INVALID_BACKUP")
}

fn inventory(root: &Path, relative: &Path, files: &mut Vec<BackupFile>) -> Result<()> {
    if relative.components().count() > 8 {
        return Err(invalid());
    }
    for entry in fs::read_dir(root.join(relative))? {
        let entry = entry?;
        let relative = relative.join(entry.file_name());
        if relative == Path::new("backup.json") {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            inventory(root, &relative, files)?;
        } else if kind.is_file() {
            if files.len() == MAX_FILES {
                return Err(invalid());
            }
            let path = relative
                .to_str()
                .filter(|s| s.len() <= 1024)
                .ok_or_else(invalid)?
                .to_owned();
            let mut source = File::open(entry.path())?;
            let mut hasher = Sha256::new();
            let mut bytes = 0;
            let mut buffer = [0; 64 * 1024];
            loop {
                let count = source.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                bytes += count as u64;
                hasher.update(&buffer[..count]);
            }
            files.push(BackupFile {
                path,
                bytes,
                sha256: hasher
                    .finalize()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect(),
            });
        } else {
            return Err(invalid());
        }
    }
    Ok(())
}

fn files(root: &Path) -> Result<Vec<BackupFile>> {
    let mut files = Vec::new();
    inventory(root, Path::new(""), &mut files)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

fn detach_links(root: &Path) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = entry.metadata()?;
        if entry.file_type()?.is_dir() {
            detach_links(&path)?;
        } else if entry.file_type()?.is_file() {
            #[cfg(unix)]
            let shared = {
                use std::os::unix::fs::MetadataExt;
                metadata.nlink() > 1
            };
            #[cfg(not(unix))]
            let shared = true;
            if shared {
                // Native checkpoints/restores hard-link SSTs; never expose those as backups.
                let temporary = path.with_extension("akasha-copy");
                let mut options = fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let mut copy = options.open(&temporary)?;
                std::io::copy(&mut File::open(&path)?, &mut copy)?;
                copy.sync_all()?;
                fs::rename(&temporary, &path)?;
            }
        } else {
            return Err(invalid());
        }
    }
    layout::sync_dir(root)
}

fn validate(root: &Path) -> Result<Manifest> {
    layout::directory(root)?;
    layout::check_format(root)?;
    let path = root.join("backup.json");
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.is_file() || metadata.len() > MAX_MANIFEST_BYTES {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(invalid());
    }
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    if manifest.schema_version != 1
        || manifest.files.len() > MAX_FILES
        || manifest.files != files(root)?
    {
        return Err(invalid());
    }
    Ok(manifest)
}

fn fresh_destination(destination: &Path, source: &Path) -> Result<PathBuf> {
    let parent = destination.parent().ok_or_else(invalid)?.canonicalize()?;
    let name = destination.file_name().ok_or_else(invalid)?;
    let destination = parent.join(name);
    if destination.starts_with(source) || source.starts_with(&destination) {
        return Err(StorageError::new("UNSAFE_STORAGE_PATH"));
    }
    layout::private_dir(&destination, false)?;
    Ok(destination)
}

impl Storage {
    /// Create a new immutable, checksummed native checkpoint. Never overwrite a destination.
    pub async fn checkpoint(&self, destination: &Path) -> Result<u64> {
        let _writer = self.writer.lock().await;
        let seq = self.sequence()?;
        let destination = fresh_destination(destination, &self.root.canonicalize()?)?;
        layout::write_new(&destination.join("format.json"), layout::FORMAT)?;
        let tree = self.tree.clone();
        let native = destination.join("kv");
        tokio::task::spawn_blocking(move || tree.create_checkpoint(native))
            .await
            .map_err(|_| StorageError::new("CHECKPOINT_FAILED"))??;
        detach_links(&destination.join("kv"))?;
        let manifest = Manifest {
            schema_version: 1,
            commit_seq: seq,
            files: files(&destination)?,
        };
        let encoded = serde_json::to_vec(&manifest)?;
        if encoded.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(invalid());
        }
        layout::write_new(&destination.join("backup.json"), &encoded)?;
        layout::sync_dir(&destination)?;
        layout::sync_dir(destination.parent().ok_or_else(invalid)?)?;
        Ok(seq)
    }

    /// Restore into a new authority directory. An unfinished restore has no valid format marker.
    pub async fn restore(config: &ResolvedConfig, backup: &Path) -> Result<Self> {
        let manifest = validate(backup)?;
        let backup = backup.canonicalize()?;
        let data = &config.config().paths.data_dir;
        layout::private_dir(data, true)?;
        let root = fresh_destination(&data.join("authority"), &backup)?;
        let store = Self {
            tree: Self::open_tree(config, &root)?,
            writer: Arc::new(Mutex::new(())),
            root,
        };
        let tree = store.tree.clone();
        let native = backup.join("kv");
        let restored =
            tokio::task::spawn_blocking(move || tree.restore_from_checkpoint(native)).await;
        let closed = store.close().await;
        restored.map_err(|_| StorageError::new("RESTORE_FAILED"))??;
        closed?;
        detach_links(&store.root.join("kv"))?;
        let store = Self {
            tree: Self::open_tree(config, &store.root)?,
            writer: Arc::new(Mutex::new(())),
            root: store.root,
        };
        let result = (|| -> Result<()> {
            // Check both sides of the native copy; published backups are immutable.
            if manifest.files != files(&backup)?
                || store.sequence()? != manifest.commit_seq
                || store.tree.begin()?.get(super::SCHEMA)?.as_deref()
                    != Some(&[super::FORMAT_VERSION])
            {
                return Err(invalid());
            }
            layout::write_new(&store.root.join("format.json"), layout::FORMAT)?;
            layout::sync_dir(&store.root)?;
            layout::sync_dir(data)?;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = store.close().await;
            return Err(error);
        }
        Ok(store)
    }
}
