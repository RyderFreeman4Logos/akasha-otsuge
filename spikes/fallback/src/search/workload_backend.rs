//! Native fallback operations for the shared workload oracle.
pub(crate) use super::Bundle;
use super::*;
use crate::{
    authority::{self, Change},
    query_fixture::Query,
    workload::hash,
};
use surrealkv::{LSMIterator, Mode};

fn projected(value: Value) -> Value {
    if value.is_null() {
        return value;
    }
    json!({"memory":value["memory"],"seq":value["seq"],"scope":value["scope"],"active":value["active"],"body":value["body"]})
}
impl Authority {
    pub(crate) async fn work_open(
        root: PathBuf,
        corpus: Option<(String, u64)>,
    ) -> Result<Self, Error> {
        if let Some((_, reserve)) = &corpus
            && fs4::available_space(&root)? < *reserve
        {
            return Err("workload disk reservation reached".into());
        }
        let authority = Self::open(&root)?;
        if let Some((manifest, _)) = corpus {
            let stored = load::read_state(&authority.tree.begin_with_mode(Mode::ReadOnly)?)?;
            if !stored.complete || stored.manifest_sha256 != manifest {
                return Err("corpus import identity is incomplete or different".into());
            }
        }
        Ok(authority)
    }
    pub(crate) async fn work_close(&self) -> Result<(), Error> {
        self.tree.close().await?;
        Ok(())
    }
    pub(crate) async fn work_get(&self, memory: u64, at: u64) -> Result<Option<Value>, Error> {
        let tx = self.tree.begin_with_mode(Mode::ReadOnly)?;
        let at = if at == i64::MAX as u64 {
            authority::point(&tx)?
        } else {
            at
        };
        self.get(&tx, memory, at)?
            .map(serde_json::to_value)
            .transpose()
            .map(|v| v.map(projected))
            .map_err(Into::into)
    }
    pub(crate) async fn work_apply(&self, change: &Change) -> Result<Value, Error> {
        self.apply(change, None).await
    }
    pub(crate) async fn work_correction_source(&self, memory: u64) -> Result<Value, Error> {
        let tx = self.tree.begin_with_mode(Mode::ReadOnly)?;
        let original = self
            .get(&tx, memory, 0)?
            .ok_or("missing original correction source")?;
        let current = self
            .get(&tx, memory, authority::point(&tx)?)?
            .ok_or("missing correction head")?;
        if !original.text_only
            || !current.text_only
            || !current.active
            || original.source_key.is_none()
            || original.source_key != current.source_key
            || original.body != current.body
            || original.scope != current.scope
        {
            return Err("corrections require restored unvectorized corpus sources".into());
        }
        Ok(
            json!({"memory":memory,"seq":current.seq,"scope":current.scope,"body":current.body,"key":current.source_key}),
        )
    }
    pub(crate) async fn work_current(&self, principal: &str, memory: u64) -> Result<Value, Error> {
        let mut result = self.current(principal, memory).await?;
        result["entry"] = projected(result["entry"].take());
        Ok(result)
    }
    pub(crate) async fn work_history(
        &self,
        principal: &str,
        scope: u8,
        at: u64,
        cursor: Option<u64>,
        limit: usize,
    ) -> Result<Value, Error> {
        let mut result = self.history(principal, scope, at, cursor, limit).await?;
        for entry in result["entries"]
            .as_array_mut()
            .ok_or("invalid history entries")?
        {
            *entry = projected(entry.take());
        }
        Ok(result)
    }
    pub(crate) async fn work_pin(&self, id: u64, manifest: &str) -> Result<Bundle, Error> {
        let base = state(&self.tree, id)?;
        if !base.ready || base.source != manifest || changes::status(self, id)?["active"] != id {
            return Err("workload generation is not active or has different corpus".into());
        }
        Ok(base.bundle)
    }
    pub(crate) async fn work_catch_up(&self, id: u64) -> Result<Value, Error> {
        let progress = changes::catch_up(self, id, 1).await?;
        let committed = progress["committed"]
            .as_u64()
            .ok_or("invalid committed progress")?;
        let text = progress["text"]["watermark"]
            .as_u64()
            .ok_or("invalid text progress")?;
        let vector = progress["vector"]["watermark"]
            .as_u64()
            .ok_or("invalid vector progress")?;
        Ok(
            json!({"generation":id,"committed":committed,"text":text,"vector":vector,"remaining":committed.saturating_sub(text.min(vector))}),
        )
    }
    pub(crate) async fn work_rebuild(&self, id: u64, active: u64) -> Result<Value, Error> {
        let candidate = state(&self.tree, id)?;
        let old = state(&self.tree, active)?;
        if id == active
            || candidate.ready
            || !old.ready
            || candidate.cursor != candidate.documents
            || candidate.pending.is_some()
            || changes::status(self, active)?["active"] != active
        {
            return Err(
                "concurrent rebuild requires an unbuilt candidate and an active old generation"
                    .into(),
            );
        }
        let start_seq = authority::point(&self.tree.begin_with_mode(Mode::ReadOnly)?)?;
        let started = Instant::now();
        let result = build_generation(self, id, false).await?;
        let end_seq = authority::point(&self.tree.begin_with_mode(Mode::ReadOnly)?)?;
        if changes::status(self, active)?["active"] != active {
            return Err("build changed the active generation".into());
        }
        Ok(
            json!({"status":"passed","generation":id,"active_generation":active,
            "start_seq":start_seq,"end_seq":end_seq,"seconds":started.elapsed().as_secs_f64(),
            "ready":result["ready"],"state":result}),
        )
    }
    pub(crate) async fn work_query(
        &self,
        query: &Query,
        vector: bool,
        principal: &str,
        generation: Option<u64>,
        bundle: Option<&Bundle>,
    ) -> Result<Value, Error> {
        let mut result = changes::query_current(
            self,
            generation.ok_or("fallback workload requires active generation")?,
            bundle.ok_or("missing workload bundle")?,
            principal,
            query.partition.as_bytes()[1] - b'0',
            (!vector).then(|| query.term.clone()),
            vector.then(|| query.embedding.clone()),
            0,
        )
        .await?;
        for hit in result["hits"]
            .as_array_mut()
            .ok_or("invalid generation hits")?
        {
            hit["key"] = hit["source_key"].clone();
        }
        Ok(result)
    }
    pub(crate) async fn work_derived_oracle(
        &self,
        id: u64,
        base: u64,
        progress: Value,
        corrections: &[u64],
    ) -> Result<Value, Error> {
        let mut filter = format!("id >= {base} AND id <= {}", base + 49);
        if !corrections.is_empty() {
            filter = format!(
                "({filter}) OR id IN ({})",
                corrections
                    .iter()
                    .map(u64::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            );
        }
        let connection = connect(&self.root).await?;
        let table = connection
            .open_table(format!("g{id}_delta_text"))
            .execute()
            .await?;
        let mut stream = table
            .query()
            .select(Select::Columns(vec![
                "id".into(),
                "revision".into(),
                "scope".into(),
                "body".into(),
            ]))
            .only_if(filter.clone())
            .execute()
            .await?;
        let mut latest: BTreeMap<u64, Value> = BTreeMap::new();
        while let Some(batch) = stream.try_next().await? {
            let ids = column::<UInt64Array>(&batch, "id")?;
            let seqs = column::<UInt64Array>(&batch, "revision")?;
            let scopes = column::<UInt8Array>(&batch, "scope")?;
            let bodies = column::<StringArray>(&batch, "body")?;
            for row in 0..batch.num_rows() {
                let memory = ids.value(row);
                let seq = seqs.value(row);
                if (!(base..=base + 49).contains(&memory) && !corrections.contains(&memory))
                    || scopes.value(row) > 7
                {
                    return Err("derived workload row outside requested range".into());
                }
                let value = json!({"memory":memory,"seq":seq,"scope":scopes.value(row),"active":true,"body_sha256":hash(bodies.value(row).as_bytes())});
                if latest
                    .get(&memory)
                    .is_none_or(|old| old["seq"].as_u64() < Some(seq))
                {
                    latest.insert(memory, value);
                }
            }
        }
        let tx = self.tree.begin_with_mode(Mode::ReadOnly)?;
        let at = authority::point(&tx)?;
        let mut rows = Vec::new();
        for (memory, value) in latest {
            let current = self
                .get(&tx, memory, at)?
                .ok_or("missing workload authority")?;
            if current.active {
                if value["seq"] != current.seq {
                    return Err("derived workload revision lags current authority".into());
                }
                rows.push(value);
            }
        }
        let prefix = format!("indexed:{id:020}:");
        let mut iterator = tx.range(
            prefix.as_bytes().to_vec(),
            format!("indexed:{id:020};").into_bytes(),
        )?;
        iterator.seek_first()?;
        let mut indexed_jobs = 0;
        while iterator.valid() {
            indexed_jobs += 1;
            iterator.next()?;
        }
        drop(iterator);
        drop(tx);
        let lifecycle = changes::status(self, id)?;
        if lifecycle["vector"]["rows"]
            .as_u64()
            .ok_or("invalid vector row count")?
            > 0
        {
            let table = connection
                .open_table(format!("g{id}_delta_vector"))
                .execute()
                .await?;
            if table.count_rows(Some(filter)).await? > 0 {
                return Err("text-only workload has derived vectors".into());
            }
        }
        Ok(json!({"progress":progress,"indexed_jobs":indexed_jobs,"text":rows,"vectors":[]}))
    }
}
