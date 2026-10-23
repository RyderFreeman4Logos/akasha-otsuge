use std::{collections::BTreeMap, io::Write, num::NonZeroUsize, path::Path, time::Duration};

use crate::engine::{Engine, Error};
use helix_db::prelude::*;
use serde_json::{Value, json};

pub async fn probe(client: &Engine) -> Result<Value, Error> {
    let rows: Vec<_> = (0..100)
        .map(|key| json!({
            "key": key,
            "body": format!("project {key} records the orbital trajectory of satellite beacon{key}"),
            "embedding": [(key as f64 + 1.0).sin(), (key as f64 + 1.0).cos(), 1.0],
            "scope": if key % 2 == 0 { "even" } else { "odd" },
        }))
        .collect();
    client.cypher(
        "UNWIND $rows AS x CREATE (:Doc {key:x.key, body:x.body, embedding:x.embedding, scope:x.scope})",
        BTreeMap::from([("rows".into(), Value::Array(rows))]),
        None,
    ).await?;
    let text = create(client, IndexSpec::node_text("Doc", "body", Some("scope"))).await?;
    let vector = create(
        client,
        IndexSpec::node_vector(
            "Doc",
            "embedding",
            NonZeroUsize::new(3).unwrap(),
            VectorDistanceMetric::Cosine,
            Some("scope"),
        ),
    )
    .await?;
    let reused = create(client, IndexSpec::node_text("Doc", "body", Some("scope"))).await?;
    assert_eq!(reused["index"]["kind"], "already_active");
    let text_hit: Value = client
        .query(QueryRequest::read(
            read_batch()
                .var_as(
                    "hits",
                    g().text_search_nodes(
                        "Doc",
                        "body",
                        "beacon42",
                        20,
                        Some(PropertyValue::from("even")),
                    )
                    .value_map(Some(vec!["key", "scope"])),
                )
                .returning(["hits"]),
        ))
        .await?;
    assert_eq!(text_hit["hits"], json!([{"key":42,"scope":"even"}]));
    let excluded: Value = client
        .query(QueryRequest::read(
            read_batch()
                .var_as(
                    "hits",
                    g().text_search_nodes(
                        "Doc",
                        "body",
                        "beacon42",
                        20,
                        Some(PropertyValue::from("odd")),
                    )
                    .value_map(Some(vec!["key"])),
                )
                .returning(["hits"]),
        ))
        .await?;
    assert_eq!(excluded["hits"], json!([]));
    let vector_hit: Value = client
        .query(QueryRequest::read(
            read_batch()
                .var_as(
                    "hits",
                    g().vector_search_nodes(
                        "Doc",
                        "embedding",
                        vec![43.0_f32.sin(), 43.0_f32.cos(), 1.0],
                        1,
                        Some(PropertyValue::from("even")),
                    )
                    .value_map(Some(vec!["key", "scope"])),
                )
                .returning(["hits"]),
        ))
        .await?;
    assert_eq!(vector_hit["hits"], json!([{"key":42,"scope":"even"}]));
    Ok(
        json!({"result":"passed", "documents":100, "fixture_dimension":3,
        "text_operation":text, "vector_operation":vector, "text":text_hit,
        "vector":vector_hit, "excluded_scope":excluded,
        "not_tested":["production_embedding_dimension","50GB","100_clients","reindex","retraction"]}),
    )
}

pub async fn create(client: &Engine, spec: IndexSpec) -> Result<Value, Error> {
    create_measured(client, spec, 7200, None).await
}

pub(crate) async fn create_measured(
    client: &Engine,
    spec: IndexSpec,
    seconds: u64,
    progress: Option<&Path>,
) -> Result<Value, Error> {
    let started = tokio::time::Instant::now();
    let receipt: Value = client
        .query(QueryRequest::write(
            write_batch()
                .var_as("index", g().create_index_if_not_exists(spec))
                .returning(["index"]),
        ))
        .await?;
    eprintln!("INDEX_RECEIPT:{receipt}");
    if receipt["index"]["kind"] == "already_active" {
        checkpoint(progress, &receipt, started, seconds)?;
        return Ok(receipt);
    }
    let operation = receipt["index"]["operation_id"]
        .as_str()
        .ok_or("no operation ID")?;
    let deadline = started + Duration::from_secs(seconds);
    let mut last_report = None;
    loop {
        let status: Value = client
            .query(QueryRequest::read(
                read_batch()
                    .var_as("operation", g().get_index_operation(operation))
                    .returning(["operation"]),
            ))
            .await?;
        let terminal = !matches!(
            status["operation"]["status"].as_str(),
            Some("queued" | "running")
        );
        if terminal
            || last_report
                .is_none_or(|last: tokio::time::Instant| last.elapsed() >= Duration::from_secs(5))
        {
            checkpoint(progress, &status, started, seconds)?;
            eprintln!("INDEX_PROGRESS:{status}");
            last_report = Some(tokio::time::Instant::now());
        }
        match status["operation"]["status"].as_str() {
            Some("succeeded") => return Ok(status),
            Some("queued" | "running") if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            _ => return Err(format!("index did not become active: {status}").into()),
        }
    }
}

fn checkpoint(
    path: Option<&Path>,
    status: &Value,
    started: tokio::time::Instant,
    seconds: u64,
) -> Result<(), Error> {
    let Some(path) = path else { return Ok(()) };
    let elapsed = started.elapsed().as_secs_f64();
    let completed = status["operation"]["progress"]["entities"]
        .as_str()
        .and_then(|v| v.parse::<u64>().ok());
    let stat = std::fs::read_to_string("/proc/self/stat")?;
    let start_ticks = stat
        .rsplit_once(')')
        .and_then(|(_, tail)| tail.split_whitespace().nth(19))
        .ok_or("invalid process identity")?;
    let receipt = json!({"phase":"index_build", "completed":completed, "total":null,
        "counter_meaning":"entity visits across scan and validation; not unique documents",
        "elapsed_s":elapsed,"rate_per_s":completed.map(|v| v as f64 / elapsed),"eta_s":null,
        "observation_deadline_s":seconds,"pid":std::process::id(),"start_ticks":start_ticks,"status":status});
    let temporary = path.with_extension("tmp");
    let mut file = std::fs::File::create(&temporary)?;
    writeln!(file, "{receipt}")?;
    file.sync_all()?;
    std::fs::rename(temporary, path)?;
    std::fs::File::open(path.parent().ok_or("progress has no parent")?)?.sync_all()?;
    Ok(())
}

pub fn build_seconds(value: Option<&str>) -> Result<u64, Error> {
    let seconds = value.unwrap_or("7200").parse()?;
    if !(1..=604800).contains(&seconds) {
        return Err("index observation seconds must be in 1..=604800".into());
    }
    Ok(seconds)
}

pub async fn build(client: &Engine, root: &Path, seconds: u64) -> Result<Value, Error> {
    let started = tokio::time::Instant::now();
    let text = create_measured(
        client,
        IndexSpec::node_text("Document", "body", Some("partition")),
        seconds,
        Some(&root.join("text-build-progress.json")),
    )
    .await?;
    eprintln!("TEXT_COMPLETE:{text}");
    let vector = create_measured(
        client,
        IndexSpec::node_vector(
            "Document",
            "embedding",
            NonZeroUsize::new(1536).unwrap(),
            VectorDistanceMetric::Cosine,
            Some("partition"),
        ),
        seconds,
        Some(&root.join("vector-build-progress.json")),
    )
    .await?;
    Ok(
        json!({"phase":"indexes_built", "text":text, "vector":vector, "elapsed_seconds":started.elapsed().as_secs_f64()}),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn observation_budget_is_explicit_and_bounded() {
        assert_eq!(super::build_seconds(None).unwrap(), 7200);
        assert_eq!(super::build_seconds(Some("172800")).unwrap(), 172800);
        for invalid in ["0", "604801", "-1", "1.2", ""] {
            assert!(super::build_seconds(Some(invalid)).is_err());
        }
    }
}
