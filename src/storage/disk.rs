use super::{Result, Storage, StorageError};
use rustix::fs::{StatVfs, statvfs};
use std::path::Path;

const CLOSEOUT_BYTES: u64 = 8 * 1024 * 1024;
const LARGE_WRITE_BYTES: usize = 16 * 1024;

fn require_space(stats: StatVfs, additional: u64) -> Result<()> {
    let available = stats.f_bavail.saturating_mul(stats.f_frsize);
    if available < CLOSEOUT_BYTES + additional {
        return Err(StorageError::new("RESOURCE_BUSY"));
    }
    Ok(())
}

impl Storage {
    fn authority_space(&self) -> Result<StatVfs> {
        Ok(statvfs(self.root.join("kv")).map_err(std::io::Error::from)?)
    }

    pub(super) fn check_write_space(&self, encoded_bytes: usize) -> Result<()> {
        if encoded_bytes >= LARGE_WRITE_BYTES {
            require_space(self.authority_space()?, encoded_bytes as u64)?;
        }
        Ok(())
    }

    pub(crate) fn check_index_space(&self, data_dir: &Path) -> Result<()> {
        require_space(self.authority_space()?, 0)?;
        // A separately mounted index must preserve its own headroom. Before its
        // first build the prospective search directory inherits the parent volume.
        let stats = match statvfs(data_dir.join("search")) {
            Err(rustix::io::Errno::NOENT) => statvfs(data_dir),
            other => other,
        };
        require_space(stats.map_err(std::io::Error::from)?, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_user_available_fragments_and_preserves_the_headroom_after_a_write() {
        let available = |additional_blocks| {
            let mut stats = statvfs(".").unwrap();
            stats.f_frsize = 4096;
            stats.f_bfree = 1_000_000;
            stats.f_bavail = CLOSEOUT_BYTES / stats.f_frsize + additional_blocks;
            stats
        };
        assert!(require_space(available(0), 0).is_ok());
        assert_eq!(
            require_space(available(0), 1).unwrap_err().code,
            "RESOURCE_BUSY"
        );
        assert!(require_space(available(4), 16 * 1024).is_ok());
        assert_eq!(
            require_space(available(4), 16 * 1024 + 1).unwrap_err().code,
            "RESOURCE_BUSY"
        );
    }
}
