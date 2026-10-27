use super::{
    RecordKey, Result, Revision, SEQUENCE, Storage, StorageError, check_id, number, read_revision,
    sequence,
};
use surrealkv::LSMIterator;

impl Storage {
    pub(crate) fn check_point(&self, at: u64) -> Result<()> {
        if at > i64::MAX as u64 || at > self.sequence()? {
            return Err(StorageError::new("INVALID_READ_POINT"));
        }
        Ok(())
    }

    pub fn recorded_at_ms(&self, seq: u64) -> Result<u64> {
        self.check_point(seq)?;
        if seq == 0 {
            return Err(StorageError::new("INVALID_READ_POINT"));
        }
        sequence(
            self.tree
                .begin()?
                .get(format!("clock/{seq:020}").as_bytes())?,
        )
    }

    fn keys(&self, start: Vec<u8>, end: Vec<u8>, limit: usize) -> Result<Vec<Vec<u8>>> {
        if limit == 0 || limit > 64 || start >= end {
            return Err(StorageError::new("INVALID_PAGE"));
        }
        let tx = self.tree.begin()?;
        let mut iter = tx.range(start, end)?;
        iter.seek_first()?;
        let mut keys = Vec::new();
        while iter.valid() && keys.len() <= limit {
            keys.push(iter.key().user_key().to_vec());
            if keys.len() > limit {
                break;
            }
            iter.next()?;
        }
        Ok(keys)
    }

    pub(crate) fn head_keys(
        &self,
        start: &RecordKey,
        end: &RecordKey,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<RecordKey>> {
        check_id(&start.id)?;
        check_id(&end.id)?;
        if start.kind != end.kind || start.id.len() != end.id.len() || start.id >= end.id {
            return Err(StorageError::new("INVALID_PAGE"));
        }
        let lower = if let Some(after) = after {
            if after.len() != start.id.len()
                || after < start.id.as_str()
                || after >= end.id.as_str()
            {
                return Err(StorageError::new("INVALID_CURSOR"));
            }
            let mut bytes = RecordKey {
                kind: start.kind.clone(),
                id: after.into(),
            }
            .head()
            .into_bytes();
            bytes.push(0);
            bytes
        } else {
            start.head().into_bytes()
        };
        let prefix_len = start.head().len() - start.id.len();
        self.keys(lower, end.head().into_bytes(), limit)?
            .into_iter()
            .map(|key| {
                let suffix = key
                    .get(prefix_len..)
                    .ok_or_else(|| StorageError::new("STORAGE_CORRUPT"))?;
                let id = std::str::from_utf8(suffix)
                    .map_err(|_| StorageError::new("STORAGE_CORRUPT"))?;
                if id.len() != start.id.len() {
                    return Err(StorageError::new("STORAGE_CORRUPT"));
                }
                Ok(RecordKey {
                    kind: start.kind.clone(),
                    id: id.into(),
                })
            })
            .collect()
    }

    pub(crate) fn revision_seqs(
        &self,
        key: &RecordKey,
        from: u64,
        to: u64,
        after: Option<u64>,
        limit: usize,
    ) -> Result<Vec<u64>> {
        check_id(&key.id)?;
        self.check_point(to)?;
        if from == 0 || from > to {
            return Err(StorageError::new("INVALID_PAGE"));
        }
        if after.is_some_and(|v| v < from || v >= to) {
            return Err(StorageError::new("INVALID_CURSOR"));
        }
        let start = after.map_or(from, |v| v + 1);
        let prefix_len = key.revision(0).len() - 20;
        self.keys(
            key.revision(start).into_bytes(),
            key.revision(to + 1).into_bytes(),
            limit,
        )?
        .into_iter()
        .map(|bytes| {
            let suffix = bytes
                .get(prefix_len..)
                .ok_or_else(|| StorageError::new("STORAGE_CORRUPT"))?;
            if suffix.len() != 20 || !suffix.iter().all(u8::is_ascii_digit) {
                return Err(StorageError::new("STORAGE_CORRUPT"));
            }
            std::str::from_utf8(suffix)
                .ok()
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| StorageError::new("STORAGE_CORRUPT"))
        })
        .collect()
    }

    /// Read the version known at a durable application commit point, including after restart.
    pub fn at(&self, key: &RecordKey, at: u64) -> Result<Option<Revision>> {
        check_id(&key.id)?;
        let tx = self.tree.begin()?;
        if at > i64::MAX as u64 || at > sequence(tx.get(SEQUENCE)?)? {
            return Err(StorageError::new("INVALID_READ_POINT"));
        }
        let seq = number(tx.get_at(key.head().as_bytes(), at + 1)?)?;
        if seq == 0 {
            return Ok(None);
        }
        if seq > at {
            return Err(StorageError::new("STORAGE_CORRUPT"));
        }
        read_revision(&tx, key, seq)?
            .map(Some)
            .ok_or_else(|| StorageError::new("STORAGE_CORRUPT"))
    }
}
