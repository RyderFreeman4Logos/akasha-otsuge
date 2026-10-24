//! Native operations for the shared, unchanged workload oracle.
use crate::{
    Error,
    authority::{self, Authority, Change},
    engine::Engine,
    generations,
    query_fixture::Query,
    search,
    workload::hash,
};
pub(crate) use generations::Bundle;
use helix_db::{HelixDbSource, prelude::*};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};

impl Authority {
    pub(crate) async fn work_open(
        root: PathBuf,
        corpus: Option<(String, u64)>,
    ) -> Result<Self, Error> {
        let engine = Engine::open_with_config(
            HelixDbSource::Disk {
                root,
                database: "akasha-storage-spike".into(),
            },
            crate::config()?,
        )
        .await?;
        if let Some((manifest, reserve)) = corpus {
            engine.require_disk_reserve(reserve)?;
            let state = engine
                .cypher(
                    "MATCH (s:ImportState) RETURN s.manifest,s.complete",
                    BTreeMap::new(),
                    None,
                )
                .await?;
            if state.rows != vec![vec![json!(manifest), json!(true)]] {
                return Err("corpus import identity is incomplete or different".into());
            }
        }
        Self::open(engine).await
    }
    pub(crate) async fn work_close(&self) -> Result<(), Error> {
        self.engine.close().await
    }
    pub(crate) async fn work_get(&self, memory: u64, at: u64) -> Result<Option<Value>, Error> {
        self.get(memory, at).await
    }
    pub(crate) async fn work_apply(&self, change: &Change) -> Result<Value, Error> {
        self.apply(change).await
    }
    pub(crate) async fn work_current(&self, principal: &str, memory: u64) -> Result<Value, Error> {
        self.current(principal, memory).await
    }
    pub(crate) async fn work_history(
        &self,
        principal: &str,
        scope: u8,
        at: u64,
        cursor: Option<u64>,
        limit: usize,
    ) -> Result<Value, Error> {
        self.state_for(principal, scope, at, cursor, limit).await
    }
    pub(crate) async fn work_pin(&self, id: u64, manifest: &str) -> Result<Bundle, Error> {
        generations::pin(self, id, manifest).await
    }
    pub(crate) async fn work_catch_up(&self, id: u64) -> Result<Value, Error> {
        generations::catch_up_one(self, id).await
    }
    pub(crate) async fn work_rebuild(&self, _id: u64, _active: u64) -> Result<Value, Error> {
        Err("concurrent rebuild is unsupported for the rejected Helix candidate".into())
    }
    pub(crate) async fn work_query(
        &self,
        query: &Query,
        vector: bool,
        principal: &str,
        generation: Option<u64>,
        bundle: Option<&Bundle>,
    ) -> Result<Value, Error> {
        if let (Some(id), Some(bundle)) = (generation, bundle) {
            let mut result = generations::query(
                self,
                id,
                bundle.clone(),
                principal,
                query.partition.as_bytes()[1] - b'0',
                vector.then(|| query.embedding.clone()),
                (!vector).then(|| query.term.clone()),
            )
            .await?;
            for hit in result["hits"]
                .as_array_mut()
                .ok_or("invalid generation hits")?
            {
                hit["key"] = hit["source_key"].clone();
            }
            Ok(result)
        } else {
            let read_seq = self.read_point().await?;
            let response = search::query(&self.engine, query, &query.partition, vector).await?;
            self.revalidate(
                principal,
                query.partition.as_bytes()[1] - b'0',
                read_seq,
                read_seq,
                response["hits"]
                    .as_array()
                    .ok_or("invalid search response")?,
            )
            .await
        }
    }
    pub(crate) async fn work_derived_oracle(
        &self,
        id: u64,
        base: u64,
        progress: Value,
    ) -> Result<Value, Error> {
        let range =
            SourcePredicate::between("memory", i64::try_from(base)?, i64::try_from(base + 49)?);
        let response = self
            .engine
            .query(QueryRequest::read(
                read_batch()
                    .var_as(
                        "text",
                        g().n_with_label_where(format!("Derived{id}Text"), range.clone())
                            .limit(51)
                            .value_map(Some(vec!["memory", "seq", "partition", "body"])),
                    )
                    .var_as(
                        "vector",
                        g().n_with_label_where(format!("Derived{id}Vector"), range)
                            .limit(1)
                            .value_map(Some(vec!["memory"])),
                    )
                    .returning(["text", "vector"]),
            ))
            .await?;
        let mut rows = match &response["text"] {
            Value::Null => vec![],
            Value::Array(rows) => rows.clone(),
            _ => return Err("invalid derived workload text".into()),
        };
        for row in &mut rows {
            let scope = row["partition"]
                .as_str()
                .ok_or("invalid derived partition")?;
            if !matches!(scope, "p0" | "p1" | "p2" | "p3" | "p4" | "p5" | "p6" | "p7") {
                return Err("invalid derived partition".into());
            }
            *row = json!({"memory":row["memory"],"seq":row["seq"],"scope":scope.as_bytes()[1]-b'0',"active":true,
                "body_sha256":hash(row["body"].as_str().ok_or("missing derived body")?.as_bytes())});
        }
        rows.sort_by_key(|row| row["memory"].as_u64());
        if !response["vector"].is_null() && response["vector"] != json!([]) {
            return Err("text-only workload has derived vectors".into());
        }
        let count = self
            .engine
            .cypher(
                "MATCH (j:IndexedJob {generation:$generation}) RETURN count(j)",
                authority::params(json!({"generation":id})),
                None,
            )
            .await?
            .rows;
        let [row] = count.as_slice() else {
            return Err("invalid indexed job count".into());
        };
        Ok(json!({"progress":progress,"indexed_jobs":row[0],"text":rows,"vectors":[]}))
    }
}
