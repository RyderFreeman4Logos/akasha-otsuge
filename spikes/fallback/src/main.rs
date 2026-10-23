mod load;
mod search;

use std::{
    io::{Read, Write},
    path::Path,
    sync::Arc,
};

use arrow_array::{
    Array, FixedSizeListArray, Int32Array, RecordBatch, RecordBatchIterator, StringArray,
    types::Float32Type,
};
use arrow_schema::{DataType, Field, Schema};
use futures::TryStreamExt;
use lance_index::scalar::FullTextSearchQuery;
use lancedb::{
    ObjectStoreRegistry, Session,
    index::{Index, scalar::FtsIndexBuilder, vector::IvfFlatIndexBuilder},
    query::{ExecutableQuery, QueryBase, Select},
};
use serde_json::{Value, json};
use surrealkv::{
    CompressionType, Durability, Options, Transaction, Tree, TreeBuilder, VLogChecksumLevel,
    WalRecoveryMode,
};

type Error = Box<dyn std::error::Error + Send + Sync>;
const OWNER: &str = "akasha-otsuge MVP-003 fallback database\n";
const MIB: usize = 1024 * 1024;
const KEYS: [&[u8]; 5] = [
    b"revision:1",
    b"head:1",
    b"receipt:req1",
    b"job:1",
    b"meta:seq",
];

async fn checkpoint(hold: Option<&str>, phase: &str) {
    if hold == Some(phase) {
        eprintln!("PROBE_CHECKPOINT:{phase}");
        std::future::pending::<()>().await;
    }
}

fn stage(tx: &mut Transaction, value: &[u8]) -> Result<(), Error> {
    tx.set_durability(Durability::Immediate);
    for key in KEYS {
        tx.set_at(key, value, 1)?;
    }
    Ok(())
}

fn text(value: Option<Vec<u8>>) -> Result<Option<String>, Error> {
    Ok(value.map(String::from_utf8).transpose()?)
}

fn open_authority(root: &Path) -> Result<Tree, Error> {
    let profile = std::env::var("AKASHA_SPIKE_PROFILE").unwrap_or_else(|_| "low".into());
    let (memtable, cache, sst) = match profile.as_str() {
        "low" => (8, 32, 8),
        "high" => (64, 128, 64),
        _ => return Err("invalid storage resource profile".into()),
    };
    let options = Options::new()
        .with_path(root.join("authority"))
        .with_max_memtable_size(memtable * MIB)
        .with_block_cache_capacity((cache * MIB) as u64)
        .with_target_file_size((sst * MIB) as u64)
        .with_compression_per_level(vec![CompressionType::ZstdCompression])
        .with_enable_vlog(true)
        .with_vlog_value_threshold(256)
        .with_vlog_max_file_size((16 * MIB) as u64)
        .with_vlog_checksum_verification(VLogChecksumLevel::Full)
        .with_memtable_stall_threshold(2)
        .with_versioning(true, 0)
        .with_wal_recovery_mode(WalRecoveryMode::AbsoluteConsistency)
        .with_flush_on_close(false);
    Ok(TreeBuilder::with_options(options).build()?)
}

async fn authority(root: &Path, operation: &str, hold: Option<&str>) -> Result<Value, Error> {
    let tree = open_authority(root)?;
    let result = match operation {
        "read" => {
            let tx = tree.begin()?;
            let values = KEYS
                .iter()
                .map(|key| text(tx.get(*key)?))
                .collect::<Result<Vec<_>, Error>>()?;
            json!({"values":values})
        }
        "write" => {
            let mut tx = tree.begin()?;
            if let Some(receipt) = tx.get_for_update(b"receipt:req1")? {
                if receipt != b"one" {
                    return Err("conflicting primitive receipt".into());
                }
                json!({"committed":true,"reused":true})
            } else {
                stage(&mut tx, b"one")?;
                checkpoint(hold, "before-commit").await;
                tx.commit().await?;
                checkpoint(hold, "after-commit").await;
                let receipt = json!({"committed":true,"reused":false});
                if hold == Some("after-reply") {
                    println!("{receipt}");
                    std::io::stdout().flush()?;
                    checkpoint(hold, "after-reply").await;
                }
                receipt
            }
        }
        "disk-full" => {
            let mut tx = tree.begin()?;
            // Exceed an existing WAL page so ENOSPC must reach the commit path.
            stage(&mut tx, &vec![b'x'; 16 * 1024])?;
            eprintln!("PROBE_CHECKPOINT:disk-full");
            let mut token = [0; 3];
            std::io::stdin().read_exact(&mut token)?;
            if token != *b"go\n" {
                return Err("invalid fault barrier release".into());
            }
            if let Err(error) = tx.commit().await {
                eprintln!("COMMIT_ERROR_PHASE");
                return Err(error.into());
            }
            json!({"committed":true})
        }
        "rollback" => {
            let mut tx = tree.begin()?;
            stage(&mut tx, b"one")?;
            tx.rollback();
            json!({"rolled_back":true})
        }
        "conflict" => {
            let mut winner = tree.begin()?;
            let mut loser = tree.begin()?;
            assert_eq!(winner.get_for_update(b"head:1")?, None);
            assert_eq!(loser.get_for_update(b"head:1")?, None);
            stage(&mut winner, b"one")?;
            stage(&mut loser, b"two")?;
            winner.commit().await?;
            assert!(matches!(
                loser.commit().await,
                Err(surrealkv::Error::TransactionWriteConflict)
            ));
            json!({"write_conflict":true})
        }
        "history-write" => {
            for (timestamp, value) in [(1, b"one"), (2, b"two")] {
                let mut tx = tree.begin()?;
                tx.set_durability(Durability::Immediate);
                tx.set_at(b"history-head", value, timestamp)?;
                tx.commit().await?;
            }
            json!({"versions":2})
        }
        "history-read" => {
            let tx = tree.begin()?;
            json!({"current":text(tx.get(b"history-head")?)?,
                "at_one":text(tx.get_at(b"history-head",1)?)?,
                "at_two":text(tx.get_at(b"history-head",2)?)?})
        }
        _ => return Err("unknown authority operation".into()),
    };
    tree.close().await?;
    Ok(result)
}

fn first_id(batches: &[RecordBatch]) -> Result<i32, Error> {
    let batch = batches
        .iter()
        .find(|b| b.num_rows() > 0)
        .ok_or("empty search result")?;
    let column = batch
        .column_by_name("id")
        .ok_or("missing result identity")?;
    Ok(column
        .as_any()
        .downcast_ref::<Int32Array>()
        .ok_or("wrong result identity type")?
        .value(0))
}

async fn search(root: &Path, create: bool) -> Result<Value, Error> {
    let uri = root.join("derived");
    let uri = uri.to_str().ok_or("non-UTF8 database path")?;
    let session = Session::new(16 * MIB, 8 * MIB, Arc::new(ObjectStoreRegistry::default()))
        .with_spill_store(Arc::new(lance_io::spill::LocalSpillStore::with_cap(
            (64 * MIB) as u64,
        )?));
    let connection = lancedb::connect(uri)
        .session(Arc::new(session))
        .execute()
        .await?;
    let table = if create {
        let vectors = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
            (0..32).map(|id| Some(vec![Some(id as f32), Some(1.0), Some(0.0)])),
            3,
        );
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("body", DataType::Utf8, false),
            Field::new("vector", vectors.data_type().clone(), true),
        ]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(Int32Array::from_iter_values(0..32)),
                Arc::new(StringArray::from_iter_values((0..32).map(|id| {
                    if id == 0 {
                        "unique astronomical pulsar"
                    } else {
                        "ordinary ocean waves"
                    }
                }))),
                Arc::new(vectors),
            ],
        )?;
        let reader = RecordBatchIterator::new(vec![Ok(batch)], schema);
        let table = connection
            .create_table(
                "documents",
                Box::new(reader) as Box<dyn arrow_array::RecordBatchReader + Send>,
            )
            .execute()
            .await?;
        table
            .create_index(
                &["body"],
                Index::FTS(
                    FtsIndexBuilder::default()
                        .memory_limit_mb(32)
                        .num_workers(1),
                ),
            )
            .execute()
            .await?;
        table
            .create_index(
                &["vector"],
                Index::IvfFlat(
                    IvfFlatIndexBuilder::default()
                        .num_partitions(1)
                        .sample_rate(16),
                ),
            )
            .execute()
            .await?;
        table
    } else {
        connection.open_table("documents").execute().await?
    };
    assert_eq!(table.list_indices().await?.len(), 2);
    let ann = table
        .query()
        .select(Select::Columns(vec!["id".into()]))
        .limit(1)
        .nearest_to(&[0.0_f32, 1.0, 0.0])?
        .execute()
        .await?
        .try_collect::<Vec<_>>()
        .await?;
    let text = table
        .query()
        .select(Select::Columns(vec!["id".into()]))
        .limit(1)
        .full_text_search(FullTextSearchQuery::new("pulsar".into()))
        .execute()
        .await?
        .try_collect::<Vec<_>>()
        .await?;
    Ok(
        json!({"rows":table.count_rows(None).await?,"ann_id":first_id(&ann)?,"text_id":first_id(&text)?}),
    )
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<(), Error> {
    let args = std::env::args().collect::<Vec<_>>();
    let operation = args
        .get(2)
        .map(String::as_str)
        .ok_or("usage: probe OWNED_ROOT OPERATION [CHECKPOINT]")?;
    let root = Path::new(&args[1]);
    if matches!(operation, "--load" | "--resume-load") {
        if args.len() != 5 {
            return Err("usage: probe ROOT --load|--resume-load INPUT MANIFEST".into());
        }
        let result = load::run(
            root,
            Path::new(&args[3]),
            Path::new(&args[4]),
            operation == "--resume-load",
        )
        .await?;
        println!("{result}");
        return Ok(());
    }
    if std::fs::read_to_string(root.join("OWNER"))? != OWNER {
        return Err("foreign probe root".into());
    }
    if matches!(operation, "derive" | "--generation") {
        if args.len() != 4 {
            return Err("usage: probe ROOT derive REQUEST.json".into());
        }
        println!("{}", search::run(root, Path::new(&args[3])).await?);
        return Ok(());
    }
    if matches!(operation, "import-state" | "verify-import" | "corpus-get") {
        if args.len() != if operation == "corpus-get" { 4 } else { 3 } {
            return Err("invalid corpus command arguments".into());
        }
        let memory = args.get(3).map(|s| s.parse::<u64>()).transpose()?;
        let result = load::inspect(root, operation, memory).await?;
        println!("{result}");
        return Ok(());
    }
    if !matches!(
        operation,
        "read"
            | "write"
            | "disk-full"
            | "rollback"
            | "conflict"
            | "history-write"
            | "history-read"
            | "search-create"
            | "search-read"
    ) {
        return Err("unknown operation".into());
    }
    let hold = args.get(3).map(String::as_str);
    if args.len() > 4
        || (hold.is_some()
            && (operation != "write"
                || !matches!(hold, Some("before-commit" | "after-commit" | "after-reply"))))
    {
        return Err("invalid checkpoint".into());
    }
    let result = if operation.starts_with("search-") {
        search(root, operation == "search-create").await?
    } else {
        authority(root, operation, hold).await?
    };
    println!("{result}");
    Ok(())
}
