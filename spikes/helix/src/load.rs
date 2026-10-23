use std::{
    error::Error,
    fs::File,
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    time::Instant,
};

use helix_db::{Client, HelixDbSource, prelude::*};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    key: String,
    body: String,
    partition: String,
    embedding: Option<Vec<f32>>,
}

pub async fn run(root: PathBuf, input: PathBuf) -> Result<(), Box<dyn Error>> {
    // Diagnostic imports require a new database: a partial import is never silently reused.
    std::fs::create_dir(&root)?;
    let client = Client::open_with_config(
        HelixDbSource::Disk {
            root,
            database: "akasha-storage-spike".into(),
        },
        super::config()?,
    )
    .await?;
    let start = Instant::now();
    let mut source = BufReader::new(File::open(input)?);
    let mut documents = 0_u64;
    let mut bytes = 0_u64;
    let mut batch = write_batch();
    loop {
        let mut line = String::new();
        let size = (&mut source).take(512 * 1024 + 1).read_line(&mut line)?;
        if size == 0 {
            break;
        }
        if size > 512 * 1024 {
            return Err("prepared record exceeds 512 KiB".into());
        }
        let doc: Document = serde_json::from_str(&line)?;
        if doc.key.is_empty()
            || doc.key.len() > 256
            || doc.body.len() > 128 * 1024
            || !["p0", "p1", "p2", "p3", "p4", "p5", "p6", "p7"].contains(&doc.partition.as_str())
        {
            return Err("invalid prepared document".into());
        }
        let body_bytes = doc.body.len() as u64;
        let mut properties = vec![
            ("key", PropertyValue::from(doc.key)),
            ("body", PropertyValue::from(doc.body)),
            ("partition", PropertyValue::from(doc.partition)),
        ];
        if let Some(vector) = doc.embedding {
            if vector.len() != 1536
                || vector.iter().any(|v| !v.is_finite())
                || vector.iter().all(|v| *v == 0.0)
            {
                return Err("invalid 1536-dimensional embedding".into());
            }
            properties.push(("embedding", PropertyValue::from(vector)));
        }
        batch = batch.var_as(
            &format!("doc{}", documents % 16),
            g().add_n("Document", properties),
        );
        documents += 1;
        bytes += body_bytes;
        if documents.is_multiple_of(16) {
            client
                .query::<Value>(QueryRequest::write(std::mem::take(&mut batch)))
                .send()
                .await?;
        }
        if documents.is_multiple_of(1000) {
            eprintln!(
                "{}",
                json!({"phase":"import","documents":documents,"content_bytes":bytes,"elapsed_seconds":start.elapsed().as_secs_f64()})
            );
        }
    }
    if !batch.entries.is_empty() {
        client
            .query::<Value>(QueryRequest::write(batch))
            .send()
            .await?;
    }
    println!(
        "{}",
        json!({"phase":"import_complete","documents":documents,"content_bytes":bytes,"elapsed_seconds":start.elapsed().as_secs_f64()})
    );
    std::io::stdout().flush()?;
    client.close().await?;
    Ok(())
}
