//! Backend-independent expected history with durable, bounded native execution.
use super::*;
use crate::search::{now_ms, remaining, start_ticks};
use std::{collections::BTreeSet, time::Instant};

include!("../../../oracle_model.rs");

fn progress(
    path: &Path,
    phase: &str,
    completed: u64,
    total: u64,
    started: Instant,
) -> Result<(), Error> {
    let elapsed = started.elapsed().as_secs_f64();
    let value = json!({"oracle_version":1,"phase":phase,"completed":completed,"total":total,
        "elapsed_s":elapsed,"rate_per_s":completed as f64/elapsed,
        "eta_s":if completed>0 {Some((total-completed) as f64*elapsed/completed as f64)} else {None},
        "pid":std::process::id(),"start_ticks":start_ticks()?,"config_epoch":1});
    let temporary = path.with_extension("json.tmp");
    let mut file = File::create(&temporary)?;
    writeln!(file, "{value}")?;
    file.sync_all()?;
    std::fs::rename(temporary, path)?;
    File::open(path.parent().ok_or("oracle checkpoint needs parent")?)?.sync_all()?;
    Ok(())
}

async fn deadline(a: &Authority, operation: &str, count: u64) -> Result<u64, Error> {
    let key = format!("oracle:{operation}:{count}:deadline");
    let mut tx = a.tree.begin()?;
    if let Some(value) = tx.get(key.as_bytes())? {
        return number(Some(value));
    }
    a.settings.check_disk(&a.root)?;
    let deadline = now_ms()? + a.settings.seconds * 1000;
    tx.set_durability(Durability::Immediate);
    tx.set(key.as_bytes(), deadline.to_be_bytes().as_slice())?;
    tx.commit().await?;
    Ok(deadline)
}

fn projected(entry: Entry) -> Value {
    json!({"memory":entry.memory,"seq":entry.seq,"scope":entry.scope,"active":entry.active,"body":entry.body})
}

async fn seed(a: &Authority, count: u64, path: &Path, until: u64) -> Result<Value, Error> {
    let started = Instant::now();
    progress(path, "oracle_seed", 0, count, started)?;
    for seq in 1..=count {
        remaining(until)?;
        let mut change = oracle_change(seq);
        change.text_only = true;
        let receipt = a.apply(&change, None).await?;
        if receipt["commit_seq"] != seq {
            return Err(format!("unexpected oracle receipt at {seq}: {receipt}").into());
        }
        if seq.is_multiple_of(1000) {
            progress(path, "oracle_seed", seq, count, started)?;
        }
    }
    progress(path, "oracle_seeded", count, count, started)?;
    Ok(
        json!({"phase":"oracle_seeded","changes":count,"version":1,"text_only":true,"seconds":started.elapsed().as_secs_f64()}),
    )
}

async fn verify(a: &Authority, count: u64, path: &Path, until: u64) -> Result<Value, Error> {
    let started = Instant::now();
    let mut first = oracle_change(1);
    first.text_only = true;
    assert_eq!(
        a.apply(&first, None).await?["commit_seq"],
        1,
        "durable receipt replay"
    );
    let mut changed = first.clone();
    changed.body.push_str(" changed");
    assert_eq!(
        a.apply(&changed, None).await?,
        json!({"conflict":"idempotency"})
    );
    changed.request_id = "stale-expectation".into();
    assert_eq!(
        a.apply(&changed, None).await?["conflict"],
        "expected_revision"
    );
    let points: BTreeSet<_> = [0, 1, count / 2, count.saturating_sub(1), count].into();
    let total = points.len() as u64 * (ORACLE_MEMORIES + 8) + 3;
    let mut checks = 0;
    progress(path, "oracle_verify", checks, total, started)?;
    for at in points {
        let tx = a.tree.begin_with_mode(Mode::ReadOnly)?;
        for memory in 0..ORACLE_MEMORIES {
            assert_eq!(
                a.get(&tx, memory, at)?.map(projected),
                oracle_state(memory, at),
                "memory={memory}, at={at}"
            );
            checks += 1;
            if memory.is_multiple_of(128) {
                remaining(until)?;
                progress(path, "oracle_verify", checks, total, started)?;
            }
        }
        for scope in 0..8 {
            remaining(until)?;
            let expected: Vec<_> = (0..ORACLE_MEMORIES)
                .filter_map(|m| oracle_state(m, at))
                .filter(|row| row["active"] == true && row["scope"] == scope)
                .collect();
            let mut actual = Vec::new();
            let mut cursor = None;
            loop {
                let page = a.page(scope, at, cursor, 100)?;
                let rows: Vec<Entry> = serde_json::from_value(page["entries"].clone())?;
                let baseline = rows.iter().any(|r| r.memory >= ORACLE_MEMORIES);
                actual.extend(
                    rows.into_iter()
                        .filter(|r| r.memory < ORACLE_MEMORIES)
                        .map(projected),
                );
                // Imported revision zero has a separate, source-byte oracle.
                if baseline {
                    break;
                }
                if let Some(next) = page["next"].as_u64() {
                    assert!(
                        cursor.is_none_or(|before| next > before),
                        "cursor must progress"
                    );
                    cursor = Some(next);
                } else {
                    break;
                }
                remaining(until)?;
            }
            assert_eq!(actual, expected, "scope={scope}, at={at}");
            checks += 1;
            progress(path, "oracle_verify", checks, total, started)?;
        }
    }
    let tx = a.tree.begin_with_mode(Mode::ReadOnly)?;
    for prefix in ["revision", "receipt", "job"] {
        let mut iter = tx.range(
            format!("{prefix}:").into_bytes(),
            format!("{prefix};").into_bytes(),
        )?;
        iter.seek_first()?;
        let mut found = 0;
        while iter.valid() {
            found += 1;
            if found % 1000 == 0 {
                remaining(until)?;
            }
            iter.next()?;
        }
        assert_eq!(found, count, "no partial or duplicate {prefix}");
        checks += 1;
    }
    progress(path, "oracle_verified", checks, total, started)?;
    Ok(
        json!({"phase":"oracle_verified","changes":count,"checks":checks,"text_only":true,
        "memory_domain":[0,ORACLE_MEMORIES],"seconds":started.elapsed().as_secs_f64(),
        "not_tested":["concurrent clients","derived watermark","live permission revocation"]}),
    )
}

pub(crate) async fn run(root: &Path, operation: &str, count: u64) -> Result<Value, Error> {
    if !(1..=1_000_000).contains(&count) {
        return Err("oracle count must be in 1..=1000000".into());
    }
    let a = Authority::open(root)?;
    let until = deadline(&a, operation, count).await?;
    remaining(until)?;
    let path = root.join(format!(
        "{}-{count}.json",
        operation.trim_start_matches("--")
    ));
    let result = if operation == "--oracle-seed-text" {
        seed(&a, count, &path, until).await
    } else {
        verify(&a, count, &path, until).await
    };
    a.tree.close().await?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn expired_original_deadline_cannot_admit_changes() -> Result<(), Error> {
        let (root, a) = crate::authority::tests::fixture()?;
        let operation = "--oracle-seed-text";
        let first = deadline(&a, operation, 2).await?;
        assert_eq!(deadline(&a, operation, 2).await?, first);
        let mut tx = a.tree.begin()?;
        tx.set_durability(Durability::Immediate);
        tx.set(
            b"oracle:--oracle-seed-text:2:deadline",
            1u64.to_be_bytes().as_slice(),
        )?;
        tx.commit().await?;
        let original = deadline(&a, operation, 2).await?;
        let result = seed(&a, 2, &root.join("test-progress.json"), original).await;
        let committed = point(&a.tree.begin_with_mode(Mode::ReadOnly)?)?;
        a.tree.close().await?;
        std::fs::remove_dir_all(root)?;
        assert_eq!(original, 1);
        assert!(result.err().is_some_and(|e| {
            e.to_string()
                .contains("original operation deadline expired")
        }));
        assert_eq!(committed, 0);
        Ok(())
    }
}
