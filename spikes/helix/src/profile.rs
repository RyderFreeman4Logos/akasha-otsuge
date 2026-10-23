use std::{
    ffi::OsStr,
    num::{NonZeroU64, NonZeroUsize},
    time::Duration,
};

use db::config::{
    CacheConfig, CacheMode, DbConfig, FtsMemoryCacheConfig, FtsWarmConfig,
    IndexLifecycleConcurrency, IndexLifecycleScanTuning, IndexLifecycleThroughputTuning,
    IndexOperationQueueTuning, QueryTelemetry, SearchIndexBackfillLimits, SearchIndexBatchLimits,
    SimHasherCacheSettings, SlateMemoryCacheConfig, SlateWarmConfig, TextBackfillCompactionLimits,
    TextBuildArtifactLimits, VectorMemoryBudget, VectorMemorySettings,
};
use serde::Serialize;

use crate::engine::Error;

const MIB: u64 = 1024 * 1024;

#[derive(Serialize)]
struct Budget {
    profile: &'static str,
    sst_compression: &'static str,
    vector_cache_mib: u64,
    slate_block_cache_mib: u64,
    slate_metadata_cache_mib: u64,
    fts_cache_mib: u64,
    vector_build_cache_mib: u64,
    unflushed_mib: usize,
    slate_subcompactions: usize,
    slate_compaction_read_ahead_kib: usize,
}

impl Budget {
    fn parse(value: Option<&OsStr>) -> Result<Self, Error> {
        match value.map(OsStr::to_str) {
            None | Some(Some("low")) => Ok(Self {
                profile: "low",
                sst_compression: "zstd",
                vector_cache_mib: 32,
                slate_block_cache_mib: 32,
                slate_metadata_cache_mib: 8,
                fts_cache_mib: 8,
                vector_build_cache_mib: 32,
                unflushed_mib: 32,
                slate_subcompactions: 1,
                slate_compaction_read_ahead_kib: 256,
            }),
            Some(Some("high")) => Ok(Self {
                profile: "high",
                sst_compression: "zstd",
                vector_cache_mib: 128,
                slate_block_cache_mib: 64,
                slate_metadata_cache_mib: 16,
                fts_cache_mib: 32,
                vector_build_cache_mib: 512,
                unflushed_mib: 64,
                slate_subcompactions: 4,
                slate_compaction_read_ahead_kib: 2048,
            }),
            _ => Err("AKASHA_SPIKE_PROFILE must be low or high".into()),
        }
    }

    fn config(&self) -> Result<DbConfig, Error> {
        let vector = VectorMemorySettings::try_new(
            VectorMemoryBudget::bounded(self.vector_cache_mib * MIB)?,
            5,
        )?
        .with_simhasher_cache(SimHasherCacheSettings::try_new(2 * MIB as usize, 4)?);
        let cache = CacheConfig::new(
            vector,
            CacheMode::Memory {
                slate_db: SlateMemoryCacheConfig::try_new(
                    self.slate_block_cache_mib * MIB,
                    self.slate_metadata_cache_mib * MIB,
                )?,
                slate_warm: SlateWarmConfig::Off,
                fts: Some(FtsMemoryCacheConfig::try_new(
                    self.fts_cache_mib * MIB,
                    FtsWarmConfig::Off,
                    300,
                )?),
            },
        );
        let positive = |bytes| NonZeroU64::new(bytes).expect("fixed positive budget");
        let queue = IndexOperationQueueTuning::default()
            .with_max_retained_bytes(positive(8 * MIB))?
            .with_max_members(positive(4096))
            .with_max_operand_bytes(positive(MIB))
            .with_strong_vector_search_max_pending_bytes(positive(8 * MIB))
            .with_strong_text_search_max_analysis_bytes(positive(16 * MIB));
        let backfill = SearchIndexBackfillLimits::try_new(
            SearchIndexBatchLimits::try_new(
                NonZeroUsize::new(512).unwrap(),
                positive(8 * MIB),
                positive(32768),
                positive(8 * MIB),
                positive(8 * MIB),
            )?,
            NonZeroUsize::MIN,
            TextBuildArtifactLimits::new(NonZeroUsize::new(512).unwrap(), positive(4 * MIB)),
            TextBackfillCompactionLimits::new(
                NonZeroUsize::new(32).unwrap(),
                positive(64 * MIB),
                positive(128 * MIB),
                positive(64 * MIB),
                positive(4 * MIB),
            ),
        )?
        .with_vector_build_cache_bytes(positive(self.vector_build_cache_mib * MIB));
        let mut slate = slatedb::Settings {
            compression_codec: Some(slatedb::config::CompressionCodec::Zstd),
            flush_interval: Some(Duration::from_millis(3)),
            l0_sst_size_bytes: 8 * MIB as usize,
            max_unflushed_bytes: self.unflushed_mib * MIB as usize,
            ..Default::default()
        };
        if let Some(worker) = slate
            .compactor_options
            .as_mut()
            .and_then(|options| options.worker.as_mut())
        {
            worker.compression_codec = slate.compression_codec;
            // Each range holds its own merge/read-ahead buffers.
            worker.max_subcompactions = self.slate_subcompactions;
            // SlateDB sizes fetches in encoded bytes but retains decoded blocks.
            worker.bytes_to_fetch = self.slate_compaction_read_ahead_kib * 1024;
        }
        Ok(DbConfig::new()
            .with_query_telemetry(QueryTelemetry::Disabled)
            .with_max_concurrent_reads(4)
            .with_slate_settings(slate)
            .with_cache(cache)
            .with_index_operation_queue_tuning(queue)
            .with_search_index_backfill_limits(backfill)
            .with_index_lifecycle_throughput_tuning(IndexLifecycleThroughputTuning::new(
                IndexLifecycleScanTuning::try_new(256 * 1024, 4)?,
                IndexLifecycleConcurrency::try_new(1, 1, 1, 1)?,
            )))
    }
}

pub fn config() -> Result<DbConfig, Error> {
    let budget = Budget::parse(std::env::var_os("AKASHA_SPIKE_PROFILE").as_deref())?;
    let config = budget.config()?;
    eprintln!("ENGINE_PROFILE:{}", serde_json::to_string(&budget)?);
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;

    #[test]
    fn absent_profile_preserves_low_and_invalid_values_fail() {
        assert_eq!(
            Budget::parse(None)
                .unwrap()
                .config()
                .unwrap()
                .cache()
                .vector_memory()
                .budget()
                .bytes(),
            Some(32 * MIB)
        );
        for value in [
            OsStr::new(""),
            OsStr::new("HIGH"),
            OsStr::from_bytes(b"\xff"),
        ] {
            assert!(Budget::parse(Some(value)).is_err());
        }
    }

    #[test]
    fn high_profile_build_cache_is_explicit_and_staging_remains_bounded() {
        let config = Budget::parse(Some(OsStr::new("high")))
            .unwrap()
            .config()
            .unwrap();
        let backfill = config.search_index_backfill();
        assert_eq!(backfill.vector_build_cache_bytes().get(), 512 * MIB);
        assert_eq!(backfill.batch().max_output_bytes().get(), 8 * MIB);
        assert_eq!(backfill.edge_property_read_batch().get(), 1);
    }

    #[test]
    fn low_profile_bounds_compaction_ranges_in_actual_writer_settings() {
        for (profile, ranges, fetch_bytes) in [("low", 1, 256 * 1024), ("high", 4, 2 * MIB as usize)] {
            let config = Budget::parse(Some(OsStr::new(profile)))
                .unwrap()
                .config()
                .unwrap();
            let settings = config.slate().to_writer_settings(None);
            let worker = settings.compactor_options.unwrap().worker.unwrap();
            assert_eq!(worker.max_subcompactions, ranges, "{profile}");
            assert_eq!(worker.max_fetch_tasks, 4);
            assert_eq!(worker.bytes_to_fetch, fetch_bytes, "{profile}");
        }
    }
}
