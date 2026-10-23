use std::{collections::BTreeMap, sync::Arc, time::Duration};

use db::{
    HelixDB, HelixDbSource,
    config::DbConfig,
    cypher::{Limits, Request, Response},
    encoding::v2::keys::scope::DataScope,
    execution_control::ExecutionControl,
    query_service::{HelixQueryService, QueryMode},
};
use helix_db::prelude::QueryRequest;
use serde_json::{Value, json};

pub type Error = Box<dyn std::error::Error + Send + Sync>;

// The SDK convenience method fixes Cypher at 256 MiB/query; use the public
// embedded request boundary to enforce the spike's smaller per-query budget.
#[derive(Clone)]
pub struct Engine {
    db: Arc<HelixDB>,
    service: HelixQueryService,
}

impl Engine {
    pub async fn open_with_config(source: HelixDbSource, config: DbConfig) -> Result<Self, Error> {
        let db = Arc::new(HelixDB::open_with_config(source, config).await?);
        Ok(Self {
            service: HelixQueryService::new(db.clone()),
            db,
        })
    }

    pub async fn cypher(
        &self,
        query: &str,
        parameters: BTreeMap<String, Value>,
        query_name: Option<&str>,
    ) -> Result<Response, Error> {
        let request: Request = serde_json::from_value(json!({
            "query": query, "parameters": parameters, "query_name": query_name,
        }))?;
        Ok(db::cypher::execute(
            &self.db,
            request,
            DataScope::LegacyUnscoped,
            QueryMode::Execute,
            ExecutionControl::from_timeout(Duration::from_secs(60)),
            Limits {
                memory_bytes: 32 * 1024 * 1024,
                result_bytes: 2 * 1024 * 1024,
                batch_rows: 64,
                collection_items: 16_384,
            },
        )
        .await?)
    }

    pub async fn query(&self, request: QueryRequest) -> Result<Value, Error> {
        let response = self
            .service
            .execute_query_with_mode_scoped_controlled(
                request,
                QueryMode::Execute,
                DataScope::LegacyUnscoped,
                ExecutionControl::from_timeout(Duration::from_secs(60)),
            )
            .await?;
        Ok(serde_json::to_value(response)?)
    }

    pub async fn close(&self) -> Result<(), Error> {
        Ok(self.db.close().await?)
    }
}
