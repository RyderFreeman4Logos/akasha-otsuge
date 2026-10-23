use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};

use db::{
    HelixDB, HelixDbSource,
    config::DbConfig,
    cypher::{Limits, Request, Response},
    encoding::v2::keys::scope::DataScope,
    execution_control::ExecutionControl,
    query_service::{HelixQueryService, QueryMode},
};
use helix_db::prelude::{IndexSpec, QueryRequest};
use serde_json::{Value, json};

pub type Error = Box<dyn std::error::Error + Send + Sync>;

// The SDK convenience method fixes Cypher at 256 MiB/query; use the public
// embedded request boundary to enforce the spike's smaller per-query budget.
#[derive(Clone)]
pub struct Engine {
    db: Arc<HelixDB>,
    service: HelixQueryService,
    disk_reserve: Option<(PathBuf, u64)>,
}

impl Engine {
    pub async fn open_with_config(source: HelixDbSource, config: DbConfig) -> Result<Self, Error> {
        let disk_reserve = match std::env::var_os("AKASHA_SPIKE_DISK_RESERVE_BYTES") {
            None => None,
            Some(value) => {
                let bytes: u64 = value.to_str().ok_or("non-UTF8 disk reservation")?.parse()?;
                if bytes < 30 * 1024u64.pow(3) + 4_000_000_000 {
                    return Err("disk reservation must retain scratch and 30 GiB".into());
                }
                let HelixDbSource::Disk { root, .. } = &source else {
                    return Err("disk reservation requires filesystem storage".into());
                };
                eprintln!("DISK_RESERVATION_BYTES:{bytes}");
                Some((root.clone(), bytes))
            }
        };
        let db = Arc::new(HelixDB::open_with_config(source, config).await?);
        Ok(Self {
            service: HelixQueryService::new(db.clone()),
            db,
            disk_reserve,
        })
    }

    pub(crate) fn admit_write(&self, input_bytes: u64) -> Result<(), Error> {
        let Some((root, reserve)) = &self.disk_reserve else {
            return Ok(());
        };
        let stat = rustix::fs::statvfs(root)?;
        let free = stat
            .f_bavail
            .checked_mul(stat.f_frsize)
            .ok_or("disk free-byte overflow")?;
        // Logical input allowance is not a proven bound on physical write amplification.
        let required = reserve
            .checked_add(input_bytes)
            .and_then(|v| v.checked_add(1024 * 1024))
            .ok_or("disk reservation overflow")?;
        if free < required {
            return Err(format!("disk reservation: {free} available, {required} required").into());
        }
        Ok(())
    }

    pub(crate) fn require_disk_reserve(&self, minimum: u64) -> Result<(), Error> {
        if self
            .disk_reserve
            .as_ref()
            .is_none_or(|(_, bytes)| *bytes < minimum)
        {
            return Err("full workload requires its retained-index disk reservation".into());
        }
        self.admit_write(0)
    }

    pub(crate) fn admit_index(&self, spec: &IndexSpec) -> Result<(), Error> {
        let catalog = self.db.index_catalog_snapshot();
        let existing = match spec {
            IndexSpec::NodeEquality {
                label, property, ..
            } => catalog
                .node_eq
                .keys()
                .any(|key| key.label.as_ref() == label && key.property.as_ref() == property),
            IndexSpec::NodeRange {
                label,
                property,
                direction,
            } => catalog.node_range.keys().any(|key| {
                key.label.as_ref() == label
                    && key.property.as_ref() == property
                    && key.direction == *direction
            }),
            _ => false,
        };
        // Native creation still validates uniqueness and exact retained identity.
        if !existing {
            self.admit_write(0)?;
        }
        Ok(())
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
