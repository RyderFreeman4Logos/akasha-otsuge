use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use surrealkv::LSMIterator;
use tokio::runtime::Runtime;

use crate::config::{self, Paths, ResolvedConfig};
use crate::storage::{Result, Storage, StorageError, import_publish::Root, layout};

fn conflict() -> StorageError {
    StorageError::new("MEMPAL_CONVERSION_CONFLICT")
}

pub(super) struct Scratch {
    admission: layout::Admission,
    path: PathBuf,
}

impl Scratch {
    pub(super) fn open(path: &Path, identity: &str) -> Result<Self> {
        let admission = layout::Admission::acquire(path, true)?;
        let owner = admission.root().join("owner.json");
        if !owner.try_exists()? {
            for entry in fs::read_dir(admission.root())? {
                if entry?.file_name() != ".archive-import.lock" {
                    return Err(conflict());
                }
            }
            super::super::writer::publish(&admission.directory, "owner.json", identity.as_bytes())?;
            admission.sync()?;
        } else {
            let mut bytes = Vec::new();
            crate::storage::import::regular_file(&owner)?
                .take(65)
                .read_to_end(&mut bytes)?;
            if bytes != identity.as_bytes() {
                return Err(conflict());
            }
        }
        let scratch = Self {
            admission,
            path: path.into(),
        };
        for entry in fs::read_dir(scratch.admission.root())? {
            let name = entry?.file_name();
            if !matches!(
                name.to_str(),
                Some("owner.json" | ".archive-import.lock" | "data")
            ) {
                return Err(conflict());
            }
        }
        // A prior interrupted run owns only this pinned, identity-bound disposable store.
        scratch.remove_data()?;
        Ok(scratch)
    }

    pub(super) fn config(&self) -> Result<ResolvedConfig> {
        config::resolve(
            "",
            None,
            "[runtime]\nmemory_budget_mib = 512\n",
            &Paths {
                data_dir: self.admission.root().join("data"),
                socket_path: self.admission.root().join("unused.sock"),
            },
        )
        .map_err(|_| StorageError::new("INVALID_STORAGE_REQUEST"))
    }

    fn remove_data(&self) -> Result<()> {
        match fs::symlink_metadata(self.admission.root().join("data")) {
            Ok(_) => Root::open(&self.admission, "data")?.remove(&self.admission, "data"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub(super) fn clean(self) -> Result<()> {
        self.remove_data()?;
        self.admission.validate()?;
        fs::remove_file(self.admission.root().join("owner.json"))?;
        fs::remove_file(self.admission.root().join(".archive-import.lock"))?;
        fs::remove_dir(&self.path)?;
        Ok(())
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RowState {
    pub origin: String,
    pub fingerprint: String,
    pub rowid: i64,
    pub present: bool,
    pub seen: bool,
    pub parts: u64,
}

pub(super) struct Index<'a> {
    pub store: Arc<Storage>,
    runtime: &'a Runtime,
    pending: BTreeMap<String, RowState>,
}

impl<'a> Index<'a> {
    pub(super) fn new(store: Arc<Storage>, runtime: &'a Runtime) -> Self {
        Self {
            store,
            runtime,
            pending: BTreeMap::new(),
        }
    }

    fn key(table: &str, key: &str) -> String {
        format!("mempal-index/{table}/{key}")
    }

    pub(super) fn get(&self, table: &str, key: &str) -> Result<Option<RowState>> {
        let key = Self::key(table, key);
        if let Some(state) = self.pending.get(&key) {
            return Ok(Some(state.clone()));
        }
        self.store
            .tree
            .begin()?
            .get(key.as_bytes())?
            .map(|value| serde_json::from_slice(&value).map_err(Into::into))
            .transpose()
    }

    pub(super) fn set(&mut self, table: &str, key: &str, state: RowState) -> Result<()> {
        self.pending.insert(Self::key(table, key), state);
        if self.pending.len() >= 128 {
            self.flush()?;
        }
        Ok(())
    }

    pub(super) fn flush(&mut self) -> Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let mut tx = self.store.tree.begin()?;
        for (key, value) in &self.pending {
            tx.set(key.as_bytes(), &serde_json::to_vec(value)?)?;
        }
        self.runtime.block_on(tx.commit())?;
        self.pending.clear();
        Ok(())
    }

    pub(super) fn missing(
        &mut self,
        table: &str,
        mut consume: impl FnMut(&str, &RowState) -> Result<()>,
    ) -> Result<()> {
        self.flush()?;
        let prefix = format!("mempal-index/{table}/");
        let tx = self.store.tree.begin()?;
        let mut iter = tx.range(
            prefix.as_bytes().to_vec(),
            format!("mempal-index/{table}0").into_bytes(),
        )?;
        iter.seek_first()?;
        while iter.valid() {
            let state: RowState = serde_json::from_slice(&iter.value()?)?;
            if state.present && !state.seen {
                let key = std::str::from_utf8(iter.key().user_key()).map_err(|_| conflict())?;
                consume(key.strip_prefix(&prefix).ok_or_else(conflict)?, &state)?;
            }
            iter.next()?;
        }
        Ok(())
    }
}
