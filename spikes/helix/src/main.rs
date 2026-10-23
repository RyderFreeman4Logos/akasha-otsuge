#![recursion_limit = "256"]

use engine::{Engine, Error};
use helix_db::HelixDbSource;
use profile::config;
use std::{io::Write, path::PathBuf};

mod authority;
mod engine;

mod indexes;
mod load;
mod profile;
mod search;
mod visibility;
mod workload;

#[tokio::main(worker_threads = 2)]
async fn main() -> Result<(), Error> {
    let args: Vec<_> = std::env::args_os().collect();
    if !(3..=5).contains(&args.len()) {
        return Err("usage: akasha-helix-probe OWNED_DATABASE_DIRECTORY CYPHER [before-execute|before-reply|after-reply]".into());
    }
    if args[2] == "--load" || args[2] == "--resume-load" {
        let input = args.get(3).ok_or("--load requires a prepared JSONL path")?;
        return load::run(
            PathBuf::from(&args[1]),
            PathBuf::from(input),
            args.get(4).map(PathBuf::from),
            args[2] == "--resume-load",
        )
        .await;
    }
    if args.len() > 4 {
        return Err("unexpected extra argument".into());
    }
    if args[2] == "--workload" {
        return workload::run(
            PathBuf::from(&args[1]),
            &PathBuf::from(args.get(3).ok_or("--workload requires a request path")?),
        )
        .await;
    }
    if args[2] == "--search-corpus" {
        let fixture = args
            .get(3)
            .ok_or("--search-corpus requires a fixture path")?;
        let client = Engine::open_with_config(
            HelixDbSource::Disk {
                root: PathBuf::from(&args[1]),
                database: "akasha-storage-spike".into(),
            },
            config()?,
        )
        .await?;
        let result = search::verify(&client, &PathBuf::from(fixture)).await;
        client.close().await?;
        println!("{}", result?);
        return Ok(());
    }
    if args[2] == "--visibility-seed" || args[2] == "--visibility-verify" {
        let fixture = args
            .get(3)
            .ok_or("visibility probe requires a query fixture")?;
        let client = Engine::open_with_config(
            HelixDbSource::Disk {
                root: PathBuf::from(&args[1]),
                database: "akasha-storage-spike".into(),
            },
            config()?,
        )
        .await?;
        let authority = authority::Authority::open(client).await?;
        let result = Box::pin(visibility::probe(
            &authority,
            &PathBuf::from(fixture),
            args[2] == "--visibility-verify",
        ))
        .await;
        authority.engine.close().await?;
        println!("{}", result?);
        return Ok(());
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
    let third = args
        .get(3)
        .map(|arg| arg.to_str().ok_or("invalid argument"))
        .transpose()?;
    let build_seconds = if args[2] == "--build-indexes" {
        Some(indexes::build_seconds(third)?)
    } else {
        None
    };
    let hold = if build_seconds.is_some() { None } else { third };
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
        let result = indexes::build(
            &client,
            &PathBuf::from(&args[1]),
            build_seconds.ok_or("missing build budget")?,
        )
        .await;
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
