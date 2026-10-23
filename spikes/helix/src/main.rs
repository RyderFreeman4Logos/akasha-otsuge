use std::{io::Write, num::NonZeroU64, path::PathBuf, time::Duration};

use db::config::{
    CacheConfig, CacheMode, DbConfig, FtsMemoryCacheConfig, FtsWarmConfig,
    IndexLifecycleConcurrency, IndexLifecycleScanTuning, IndexLifecycleThroughputTuning,
    IndexOperationQueueTuning, QueryTelemetry, SearchIndexBackfillLimits, SimHasherCacheSettings,
    SlateMemoryCacheConfig, SlateWarmConfig, VectorMemoryBudget, VectorMemorySettings,
};
use engine::{Engine, Error};
use helix_db::HelixDbSource;

mod authority;
mod engine;

mod indexes;
mod load;

const MIB: u64 = 1024 * 1024;

fn config() -> Result<DbConfig, Error> {
    let vector = VectorMemorySettings::try_new(VectorMemoryBudget::bounded(32 * MIB)?, 5)?
        .with_simhasher_cache(SimHasherCacheSettings::try_new(2 * MIB as usize, 4)?);
    let cache = CacheConfig::new(
        vector,
        CacheMode::Memory {
            slate_db: SlateMemoryCacheConfig::try_new(32 * MIB, 8 * MIB)?,
            slate_warm: SlateWarmConfig::Off,
            fts: Some(FtsMemoryCacheConfig::try_new(
                8 * MIB,
                FtsWarmConfig::Off,
                300,
            )?),
        },
    );
    let queue = IndexOperationQueueTuning::default()
        .with_max_retained_bytes(NonZeroU64::new(8 * MIB).unwrap())?
        .with_max_members(NonZeroU64::new(4096).unwrap())
        .with_max_operand_bytes(NonZeroU64::new(MIB).unwrap())
        .with_strong_vector_search_max_pending_bytes(NonZeroU64::new(8 * MIB).unwrap())
        .with_strong_text_search_max_analysis_bytes(NonZeroU64::new(16 * MIB).unwrap());
    let slate = slatedb::Settings {
        flush_interval: Some(Duration::from_millis(3)),
        l0_sst_size_bytes: 8 * MIB as usize,
        max_unflushed_bytes: 32 * MIB as usize,
        ..Default::default()
    };
    Ok(DbConfig::new()
        .with_query_telemetry(QueryTelemetry::Disabled)
        .with_max_concurrent_reads(4)
        .with_slate_settings(slate)
        .with_cache(cache)
        .with_index_operation_queue_tuning(queue)
        .with_search_index_backfill_limits(
            SearchIndexBackfillLimits::default()
                .with_vector_build_cache_bytes(NonZeroU64::new(32 * MIB).unwrap()),
        )
        .with_index_lifecycle_throughput_tuning(IndexLifecycleThroughputTuning::new(
            IndexLifecycleScanTuning::default(),
            IndexLifecycleConcurrency::try_new(1, 1, 1, 1)?,
        )))
}

#[tokio::main(worker_threads = 2)]
async fn main() -> Result<(), Error> {
    let args: Vec<_> = std::env::args_os().collect();
    if !(3..=4).contains(&args.len()) {
        return Err("usage: akasha-helix-probe OWNED_DATABASE_DIRECTORY CYPHER [before-execute|before-reply|after-reply]".into());
    }
    if args[2] == "--load" {
        let input = args.get(3).ok_or("--load requires a prepared JSONL path")?;
        return load::run(PathBuf::from(&args[1]), PathBuf::from(input)).await;
    }
    if args[2] == "--oracle-seed" || args[2] == "--oracle-verify" {
        let count = args
            .get(3)
            .and_then(|v| v.to_str())
            .ok_or("oracle operation requires count")?
            .parse()?;
        let client = Engine::open_with_config(
            HelixDbSource::Disk {
                root: PathBuf::from(&args[1]),
                database: "akasha-storage-spike".into(),
            },
            config()?,
        )
        .await?;
        let authority = authority::Authority::open(client).await?;
        let progress = PathBuf::from(&args[1]).join(format!(
            "{}-{count}.json",
            args[2].to_string_lossy().trim_start_matches('-')
        ));
        let result = if args[2] == "--oracle-seed" {
            authority::seed(&authority, count, &progress).await
        } else {
            authority::verify(&authority, count, &progress).await
        };
        authority.engine.close().await?;
        println!("{}", result?);
        return Ok(());
    }
    let hold = args
        .get(3)
        .map(|arg| arg.to_str().ok_or("invalid phase"))
        .transpose()?;
    if hold.is_some_and(|value| !["before-execute", "before-reply", "after-reply"].contains(&value))
    {
        return Err("invalid phase".into());
    }
    let query = args[2].to_str().ok_or("query is not UTF-8")?;
    if query.len() > 64 * 1024 {
        return Err("probe query exceeds 64 KiB".into());
    }
    let client = Engine::open_with_config(
        HelixDbSource::Disk {
            root: PathBuf::from(&args[1]),
            database: "akasha-storage-spike".into(),
        },
        config()?,
    )
    .await?;
    if query == "--indexes" {
        let result = indexes::probe(&client).await;
        client.close().await?;
        println!("{}", serde_json::to_string(&result?)?);
        return Ok(());
    }
    if query == "--build-indexes" {
        let result = indexes::build(&client).await;
        client.close().await?;
        println!("{}", serde_json::to_string(&result?)?);
        return Ok(());
    }
    checkpoint(hold, "before-execute").await;
    let result = client.cypher(query, Default::default(), None).await;
    let result = match result {
        Ok(value) => value,
        Err(error) => {
            client.close().await?;
            return Err(error);
        }
    };
    checkpoint(hold, "before-reply").await;
    println!("{}", serde_json::to_string(&result)?);
    std::io::stdout().flush()?;
    checkpoint(hold, "after-reply").await;
    client.close().await?;
    Ok(())
}

async fn checkpoint(requested: Option<&str>, phase: &str) {
    if requested == Some(phase) {
        eprintln!("PROBE_CHECKPOINT:{phase}");
        std::future::pending::<()>().await;
    }
}
