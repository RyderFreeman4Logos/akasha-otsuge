use super::*;
use std::collections::BTreeMap;

/// Bounds Arrow buffers while coalescing repeated operations on the same native key.
pub(super) struct NativeBatch {
    rows: BTreeMap<String, Option<RecordBatch>>,
    bytes: usize,
    limit: usize,
}

impl NativeBatch {
    pub(super) fn new(budget_mib: u32) -> Self {
        Self {
            rows: BTreeMap::new(),
            bytes: 0,
            limit: (budget_mib as usize * 1024 * 1024 / 32).clamp(128 * 1024, 4 * 1024 * 1024),
        }
    }

    pub(super) async fn push(
        &mut self,
        table: &Table,
        key: String,
        row: Option<RecordBatch>,
    ) -> Result<()> {
        self.bytes += row.as_ref().map_or(0, RecordBatch::get_array_memory_size);
        if let Some(Some(previous)) = self.rows.insert(key, row) {
            self.bytes -= previous.get_array_memory_size();
        }
        if self.bytes >= self.limit || self.rows.len() >= 256 {
            self.flush(table).await?;
        }
        Ok(())
    }

    pub(super) async fn flush(&mut self, table: &Table) -> Result<()> {
        let mut upserts = Vec::new();
        let mut removals = Vec::new();
        for (key, row) in std::mem::take(&mut self.rows) {
            match row {
                Some(row) => upserts.push(Ok(row)),
                // Both callers generate keys with SHA-256, never raw user text.
                None => removals.push(format!("'{key}'")),
            }
        }
        self.bytes = 0;
        if !removals.is_empty() {
            table
                .delete(&format!("key IN ({})", removals.join(",")))
                .await
                .map_err(index_error)?;
        }
        if let Some(Ok(first)) = upserts.first() {
            let schema = first.schema();
            let mut merge = table.merge_insert(&["key"]);
            merge
                .when_matched_update_all(None)
                .when_not_matched_insert_all();
            merge
                .execute(Box::new(RecordBatchIterator::new(upserts, schema)))
                .await
                .map_err(index_error)?;
        }
        Ok(())
    }
}
