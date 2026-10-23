use std::{collections::BTreeMap, error::Error, num::NonZeroUsize, time::Duration};

use helix_db::{Client, prelude::*};
use serde_json::{Value, json};

pub async fn probe(client: &Client) -> Result<Value, Box<dyn Error>> {
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
        .send()
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
        .send()
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
        .send()
        .await?;
    assert_eq!(vector_hit["hits"], json!([{"key":42,"scope":"even"}]));
    Ok(
        json!({"result":"passed", "documents":100, "fixture_dimension":3,
        "text_operation":text, "vector_operation":vector, "text":text_hit,
        "vector":vector_hit, "excluded_scope":excluded,
        "not_tested":["production_embedding_dimension","50GB","100_clients","reindex","retraction"]}),
    )
}

pub async fn create(client: &Client, spec: IndexSpec) -> Result<Value, Box<dyn Error>> {
    let receipt: Value = client
        .query(QueryRequest::write(
            write_batch()
                .var_as("index", g().create_index_if_not_exists(spec))
                .returning(["index"]),
        ))
        .send()
        .await?;
    eprintln!("INDEX_RECEIPT:{receipt}");
    if receipt["index"]["kind"] == "already_active" {
        return Ok(receipt);
    }
    let operation = receipt["index"]["operation_id"]
        .as_str()
        .ok_or("no operation ID")?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(7200);
    let mut last_report = tokio::time::Instant::now();
    loop {
        let status: Value = client
            .query(QueryRequest::read(
                read_batch()
                    .var_as("operation", g().get_index_operation(operation))
                    .returning(["operation"]),
            ))
            .send()
            .await?;
        match status["operation"]["status"].as_str() {
            Some("succeeded") => return Ok(status),
            Some("queued" | "running") if tokio::time::Instant::now() < deadline => {
                if last_report.elapsed() >= Duration::from_secs(5) {
                    eprintln!("INDEX_PROGRESS:{status}");
                    last_report = tokio::time::Instant::now();
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            _ => return Err(format!("index did not become active: {status}").into()),
        }
    }
}

pub async fn build(client: &Client) -> Result<Value, Box<dyn Error>> {
    let started = tokio::time::Instant::now();
    let text = create(
        client,
        IndexSpec::node_text("Document", "body", Some("partition")),
    )
    .await?;
    eprintln!("TEXT_COMPLETE:{text}");
    let vector = create(
        client,
        IndexSpec::node_vector(
            "Document",
            "embedding",
            NonZeroUsize::new(1536).unwrap(),
            VectorDistanceMetric::Cosine,
            Some("partition"),
        ),
    )
    .await?;
    Ok(
        json!({"phase":"indexes_built", "text":text, "vector":vector, "elapsed_seconds":started.elapsed().as_secs_f64()}),
    )
}
