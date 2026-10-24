//! Durable outbox completion follows native Lance publication, never precedes it.
use super::*;
use crate::authority::{Entry, point};
use lance_index::{
    optimize::OptimizeOptions,
    scalar::inverted::builder::{LANCE_FTS_NUM_SHARDS, LANCE_FTS_PARTITION_SIZE},
};
use surrealkv::{Mode, Transaction};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Family {
    Text,
    Vector,
}
impl Family {
    fn name(self) -> &'static str {
        if self == Self::Text { "text" } else { "vector" }
    }
    fn table(self, id: u64) -> String {
        format!("g{id}_delta_{}", self.name())
    }
    fn state_key(self, id: u64) -> String {
        format!("outbox:{id:020}:{}", self.name())
    }
    fn completion_key(self, id: u64, seq: u64) -> String {
        format!("indexed:{id:020}:{}:{seq:020}", self.name())
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct VectorOutput {
    model: String,
    model_fingerprint: String,
    body_sha256: String,
    values: Vec<f32>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Completion {
    seq: u64,
    hash: String,
    reason: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    seqs: Vec<u64>,
    hashes: Vec<String>,
    reasons: Vec<String>,
    payload_hash: String,
    deadline_ms: u64,
    appended_version: Option<u64>,
}
#[derive(Default, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Applied {
    watermark: u64,
    rows: u64,
    version: u64,
    pending: Option<Pending>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CatchUp {
    target: u64,
    deadline_ms: u64,
    done: bool,
}

fn hash(bytes: impl AsRef<[u8]>) -> String {
    load::hex(&Sha256::new_with_prefix(bytes))
}
fn applied(tx: &Transaction, id: u64, family: Family) -> Result<Applied, Error> {
    let value: Applied = tx
        .get(family.state_key(id).as_bytes())?
        .map(|v| serde_json::from_slice(&v))
        .transpose()?
        .unwrap_or_default();
    if value.watermark > point(tx)?
        || value.rows > point(tx)?
        || value.pending.as_ref().is_some_and(|p| {
            p.seqs.is_empty()
                || p.seqs.len() > 256
                || p.seqs.len() != p.hashes.len()
                || p.seqs.len() != p.reasons.len()
                || !p.seqs.windows(2).all(|s| s[0] < s[1])
        })
    {
        return Err("invalid outbox progress".into());
    }
    Ok(value)
}
fn completed(
    tx: &Transaction,
    id: u64,
    family: Family,
    seq: u64,
) -> Result<Option<Completion>, Error> {
    let value: Option<Completion> = tx
        .get(family.completion_key(id, seq).as_bytes())?
        .map(|v| serde_json::from_slice(&v))
        .transpose()?;
    if value.as_ref().is_some_and(|c| {
        c.seq != seq
            || c.hash.len() != 64
            || !matches!(
                c.reason.as_str(),
                "applied" | "superseded" | "retracted" | "not_required"
            )
            || (c.reason == "not_required" && family != Family::Vector)
    }) {
        return Err("invalid derived completion".into());
    }
    Ok(value)
}
async fn save_progress(
    a: &Authority,
    id: u64,
    family: Family,
    value: &Applied,
) -> Result<(), Error> {
    a.settings.check_disk(&a.root)?;
    let mut tx = a.tree.begin()?;
    tx.set_durability(Durability::Immediate);
    tx.set(family.state_key(id).as_bytes(), serde_json::to_vec(value)?)?;
    tx.commit().await?;
    Ok(())
}
fn checked_output(
    bundle: &Bundle,
    family: Family,
    entry: &Entry,
    output: Option<&VectorOutput>,
) -> Result<(), Error> {
    if family == Family::Text && output.is_some() {
        return Err("text job cannot consume vector output".into());
    }
    if entry.text_only && output.is_some() {
        return Err("text-only source cannot consume vector output".into());
    }
    if let Some(v) = output
        && (v.model != bundle.model
            || v.model_fingerprint != bundle.model_fingerprint
            || v.body_sha256 != hash(entry.body.as_bytes())
            || v.values.len() != bundle.dimension
            || v.values.iter().any(|v| !v.is_finite())
            || v.values.iter().all(|v| *v == 0.0))
    {
        return Err("embedding bundle or source identity mismatch".into());
    }
    Ok(())
}
fn data(
    entries: &[Entry],
    completions: &[Completion],
    family: Family,
    output: Option<&VectorOutput>,
) -> Result<RecordBatch, Error> {
    let selected: Vec<_> = entries
        .iter()
        .zip(completions)
        .filter(|(_, c)| c.reason == "applied")
        .map(|(e, _)| e)
        .collect();
    let docs: Vec<_> = selected
        .iter()
        .map(|e| {
            (
                e.memory,
                load::Document {
                    key: e
                        .source_key
                        .clone()
                        .unwrap_or_else(|| format!("revision:{}", e.seq)),
                    body: e.body.clone(),
                    partition: format!("p{}", e.scope),
                    embedding: if family == Family::Vector {
                        output.map(|v| v.values.clone())
                    } else {
                        None
                    },
                },
            )
        })
        .collect();
    let batch = super::batch(&docs, family == Family::Vector, TextLayout::Legacy)?;
    let mut columns = batch.columns().to_vec();
    columns[2] = Arc::new(UInt64Array::from_iter_values(
        selected.iter().map(|e| e.seq),
    ));
    Ok(RecordBatch::try_new(batch.schema(), columns)?)
}
async fn refresh(
    table: &Table,
    a: &Authority,
    family: Family,
    rows: u64,
    until: u64,
) -> Result<(), Error> {
    let search: (&str, &str, IndexType, Box<dyn IndexParams>) = if family == Family::Text {
        (
            "body_fts",
            "body",
            IndexType::Inverted,
            Box::new(
                FtsIndexBuilder::default()
                    .memory_limit_mb(32)
                    .num_workers(1)
                    .with_position(false),
            ),
        )
    } else {
        let mut ivf = IvfBuildParams::new(((rows as f64).sqrt() as usize).clamp(1, 256));
        ivf.sample_rate = 16;
        ivf.max_iters = 20;
        (
            "vector_ivf",
            "vector",
            IndexType::Vector,
            Box::new(VectorIndexParams::with_ivf_flat_params(
                lancedb::DistanceType::Cosine.into(),
                ivf,
            )),
        )
    };
    for (name, column, kind, params) in [
        (
            "scope_bitmap",
            "scope",
            IndexType::Bitmap,
            Box::new(ScalarIndexParams::for_builtin(BuiltinIndexType::Bitmap))
                as Box<dyn IndexParams>,
        ),
        search,
    ] {
        let existing = table.index_stats(name).await?;
        if existing
            .as_ref()
            .is_some_and(|s| s.num_indexed_rows as u64 == rows && s.num_unindexed_rows == 0)
        {
            continue;
        }
        a.settings.check_disk(&a.root)?;
        remaining(until)?;
        let wrapper = table.dataset().ok_or("local delta dataset required")?;
        let mut dataset = (*wrapper.get().await?).clone();
        let progress = Arc::new(Progress {
            path: a
                .root
                .join(format!("{}-{name}-progress.json", table.name())),
            stages: Mutex::new(BTreeMap::new()),
            settings: a.settings,
            deadline: until,
            start_ticks: start_ticks()?,
        });
        if kind != IndexType::Vector && existing.is_some() {
            // Lance does not persist build-time limits in FTS metadata.
            if kind == IndexType::Inverted
                && (*LANCE_FTS_NUM_SHARDS != 1 || *LANCE_FTS_PARTITION_SIZE != 32)
            {
                return Err(
                    "incremental FTS requires LANCE_FTS_NUM_SHARDS=1 and LANCE_FTS_PARTITION_SIZE=32"
                        .into(),
                );
            }
            dataset
                .optimize_indices(
                    &OptimizeOptions::merge(1)
                        .index_names(vec![name.into()])
                        .progress(progress),
                )
                .await?;
        } else {
            dataset
                .create_index_builder(&[column], kind, params.as_ref())
                .name(name.into())
                .replace(true)
                .progress(progress)
                .await?;
        }
        wrapper.update(dataset);
        let stats = table
            .index_stats(name)
            .await?
            .ok_or("missing delta index")?;
        if stats.num_indexed_rows as u64 != rows || stats.num_unindexed_rows != 0 {
            return Err("incomplete delta index coverage".into());
        }
    }
    Ok(())
}

pub(super) async fn apply(
    a: &Authority,
    id: u64,
    family: Family,
    seqs: &[u64],
    output: Option<&VectorOutput>,
    hold: Option<&str>,
    catchup: bool,
) -> Result<Value, Error> {
    if seqs.is_empty()
        || seqs.len() > 256
        || !seqs.windows(2).all(|s| s[0] < s[1])
        || (seqs.len() > 1 && output.is_some())
    {
        return Err("invalid outbox wave".into());
    }
    if hold.is_some_and(|v| {
        !matches!(
            v,
            "before-delta-append"
                | "after-delta-append"
                | "after-index-application"
                | "before-watermark"
        )
    }) {
        return Err("invalid application checkpoint".into());
    }
    let _derived = a.derived.lock().await;
    let base = state(&a.tree, id)?;
    if !base.ready {
        return Err("generation not ready for derived writes".into());
    }
    let tx = a.tree.begin_with_mode(Mode::ReadOnly)?;
    let at = point(&tx)?;
    let mut progress = applied(&tx, id, family)?;
    let mut entries = Vec::new();
    let mut completions = Vec::new();
    for &seq in seqs {
        let entry = a.job_entry(&tx, seq)?;
        checked_output(&base.bundle, family, &entry, output)?;
        let fingerprint = hash(serde_json::to_vec(
            &json!({"generation":id,"bundle":base.bundle,"family":family,"seq":seq,"vector":output}),
        )?);
        if let Some(prior) = completed(&tx, id, family, seq)? {
            if prior.hash != fingerprint && !catchup {
                return Err("indexed job idempotency conflict".into());
            }
            continue;
        }
        let current = a
            .get(&tx, entry.memory, at)?
            .ok_or("missing authoritative head")?;
        let mut reason = if current.seq > seq {
            "superseded"
        } else if !entry.active {
            "retracted"
        } else if entry.text_only && family == Family::Vector {
            "not_required"
        } else {
            "applied"
        }
        .to_string();
        if current.seq < seq {
            return Err("outbox source is ahead of its head".into());
        }
        if let Some(p) = &progress.pending
            && let Some(position) = p.seqs.iter().position(|v| *v == seq)
        {
            if p.hashes[position] != fingerprint {
                return Err("pending application identity conflict".into());
            }
            reason = p.reasons[position].clone();
        }
        if reason == "applied" && family == Family::Vector && output.is_none() {
            return Err("embedding output required".into());
        }
        completions.push(Completion {
            seq,
            hash: fingerprint,
            reason,
        });
        entries.push(entry);
    }
    drop(tx);
    if completions.is_empty() {
        advance_locked(a, id, family).await?;
        return Ok(json!({"generation":id,"family":family,"reused":true}));
    }
    let pending_seqs: Vec<_> = completions.iter().map(|c| c.seq).collect();
    let hashes: Vec<_> = completions.iter().map(|c| c.hash.clone()).collect();
    let payload_hash = hash(serde_json::to_vec(&(&entries, &hashes))?);
    if let Some(p) = &progress.pending {
        if p.seqs != pending_seqs || p.hashes != hashes || p.payload_hash != payload_hash {
            return Err("another outbox wave is pending".into());
        }
    } else {
        progress.pending = Some(Pending {
            seqs: pending_seqs,
            hashes,
            reasons: completions.iter().map(|c| c.reason.clone()).collect(),
            payload_hash,
            deadline_ms: now_ms()? + a.settings.seconds * 1000,
            appended_version: None,
        });
        save_progress(a, id, family, &progress).await?;
    }
    let pending = progress.pending.clone().ok_or("missing pending wave")?;
    remaining(pending.deadline_ms)?;
    let rows = data(&entries, &completions, family, output)?;
    if rows.num_rows() > 0 {
        let connection = connect(a).await?;
        let name = family.table(id);
        if pending.appended_version.is_none() {
            let tag = format!("outbox:{id}:{}:{}", family.name(), pending.payload_hash);
            checkpoint(hold, "before-delta-append").await;
            let version = append(
                &connection,
                &name,
                rows.clone(),
                progress.rows,
                progress.version,
                &tag,
            )
            .await?;
            checkpoint(hold, "after-delta-append").await;
            progress
                .pending
                .as_mut()
                .ok_or("missing pending wave")?
                .appended_version = Some(version);
            save_progress(a, id, family, &progress).await?;
        }
        let table = connection.open_table(&name).execute().await?;
        if table.count_rows(None).await? as u64 != progress.rows + rows.num_rows() as u64 {
            return Err("delta row coverage mismatch".into());
        }
        verify_appended(&table, &rows, progress.rows).await?;
        tokio::time::timeout(
            remaining(pending.deadline_ms)?,
            refresh(
                &table,
                a,
                family,
                progress.rows + rows.num_rows() as u64,
                pending.deadline_ms,
            ),
        )
        .await
        .map_err(|_| "original operation deadline expired")??;
        progress.version = table.version().await?;
        progress.rows += rows.num_rows() as u64;
    }
    checkpoint(hold, "after-index-application").await;
    #[cfg(test)]
    if let Some((entered, resume)) = &a.derived_barrier {
        entered.notify_one();
        resume.notified().await;
    }
    a.settings.check_disk(&a.root)?;
    let mut tx = a.tree.begin()?;
    tx.set_durability(Durability::Immediate);
    for c in &completions {
        tx.set(
            family.completion_key(id, c.seq).as_bytes(),
            serde_json::to_vec(c)?,
        )?;
    }
    progress.pending = None;
    tx.set(
        family.state_key(id).as_bytes(),
        serde_json::to_vec(&progress)?,
    )?;
    tx.commit().await?;
    checkpoint(hold, "before-watermark").await;
    let advanced = advance_locked(a, id, family).await?;
    Ok(
        json!({"generation":id,"family":family,"completed":completions.len(),"reason":if completions.len()==1 {Some(&completions[0].reason)} else {None},"progress":advanced,"reused":false}),
    )
}
async fn advance_locked(a: &Authority, id: u64, family: Family) -> Result<Value, Error> {
    let mut tx = a.tree.begin()?;
    let key = family.state_key(id);
    tx.get_for_update(key.as_bytes())?;
    let mut value = applied(&tx, id, family)?;
    let before = value.watermark;
    let at = point(&tx)?;
    for seq in before + 1..=at.min(before.saturating_add(256)) {
        if completed(&tx, id, family, seq)?.is_none() {
            break;
        }
        a.job_entry(&tx, seq)?;
        value.watermark = seq;
    }
    if value.watermark != before {
        a.settings.check_disk(&a.root)?;
        tx.set_durability(Durability::Immediate);
        tx.set(key.as_bytes(), serde_json::to_vec(&value)?)?;
        tx.commit().await?;
    }
    Ok(json!({"before":before,"watermark":value.watermark}))
}
pub(super) async fn advance(a: &Authority, id: u64, family: Family) -> Result<Value, Error> {
    let _derived = a.derived.lock().await;
    state(&a.tree, id)?;
    advance_locked(a, id, family).await
}
pub(super) fn status(a: &Authority, id: u64) -> Result<Value, Error> {
    state(&a.tree, id)?;
    let tx = a.tree.begin_with_mode(Mode::ReadOnly)?;
    let active = tx
        .get(b"active:generation")?
        .map(|v| serde_json::from_slice::<u64>(&v))
        .transpose()?
        .unwrap_or(0);
    Ok(
        json!({"active":active,"committed":point(&tx)?,"text":applied(&tx,id,Family::Text)?,"vector":applied(&tx,id,Family::Vector)?}),
    )
}

pub(super) async fn prune_versions(
    a: &Authority,
    id: u64,
    until: u64,
    hold: Option<&str>,
) -> Result<Value, Error> {
    if hold.is_some_and(|v| !matches!(v, "before-prune" | "after-prune-table")) {
        return Err("invalid prune checkpoint".into());
    }
    let base = state(&a.tree, id)?;
    let before = status(a, id)?;
    if !base.ready || before["active"] != id {
        return Err("prune requires the active ready generation".into());
    }
    let tx = a.tree.begin_with_mode(Mode::ReadOnly)?;
    let at = point(&tx)?;
    let mut names = vec![(table_name(id, false), base.documents)];
    if base.vectors > 0 {
        names.push((table_name(id, true), base.vectors));
    }
    for family in [Family::Text, Family::Vector] {
        let p = applied(&tx, id, family)?;
        if p.pending.is_some() || p.watermark != at {
            return Err("prune requires drained outbox".into());
        }
        if p.rows > 0 {
            names.push((family.table(id), p.rows));
        }
    }
    drop(tx);
    let connection = connect(a).await?;
    let mut tables = Vec::new();
    for (name, rows) in names {
        let table = connection.open_table(name).execute().await?;
        if table.count_rows(None).await? as u64 != rows {
            return Err("prune row coverage mismatch".into());
        }
        let version = table.version().await?;
        tables.push((table, version, rows));
    }
    let total = tables.len();
    let started = Instant::now();
    let ticks = start_ticks()?;
    let progress = |reports: &[Value]| {
        let elapsed = started.elapsed().as_secs_f64();
        load::atomic_json(
            &a.root.join(format!("generation-{id}-prune.json")),
            &json!({"phase":"prune_versions","generation":id,"deadline_ms":until,
                "pid":std::process::id(),"start_ticks":ticks,"completed":reports.len(),"total":total,
                "elapsed_s":elapsed,"rate_per_s":reports.len() as f64/elapsed.max(0.001),
                "eta_s":if reports.is_empty(){None}else{Some((total-reports.len()) as f64*elapsed/reports.len() as f64)},
                "tables":reports}),
        )
    };
    let mut reports = Vec::new();
    progress(&reports)?;
    checkpoint(hold, "before-prune").await;
    for (table, version, rows) in tables {
        remaining(until)?;
        // This single-request CLI holds the native authority lock; no generation reader or writer can overlap.
        let stats = table
            .optimize(lancedb::table::OptimizeAction::Prune {
                older_than: Some(lancedb::table::Duration::zero()),
                delete_unverified: Some(true),
                error_if_tagged_old_versions: Some(true),
            })
            .await?
            .prune
            .ok_or("missing native prune result")?;
        let reopened = connection.open_table(table.name()).execute().await?;
        if reopened.version().await? != version || reopened.count_rows(None).await? as u64 != rows {
            return Err("prune changed the current table".into());
        }
        reports.push(json!({"table":table.name(),"version":version,"rows":rows,
            "bytes_removed":stats.bytes_removed,"old_versions":stats.old_versions,
            "data_files_removed":stats.data_files_removed,"transaction_files_removed":stats.transaction_files_removed,
            "index_files_removed":stats.index_files_removed,"deletion_files_removed":stats.deletion_files_removed,
            "failed_deletes":stats.failed_deletes}));
        progress(&reports)?;
        if stats.failed_deletes != 0 {
            return Err("native prune left failed deletions".into());
        }
        checkpoint(hold, "after-prune-table").await;
    }
    if status(a, id)? != before {
        return Err("prune changed authority or watermarks".into());
    }
    Ok(json!({"generation":id,"deadline_ms":until,"tables":reports,"lifecycle":before}))
}

pub(super) async fn retire_generation(
    a: &Authority,
    id: u64,
    until: u64,
    hold: Option<&str>,
) -> Result<Value, Error> {
    if hold.is_some_and(|v| {
        !matches!(
            v,
            "before-retire" | "after-retire-intent" | "after-retire-table"
        )
    }) {
        return Err("invalid retirement checkpoint".into());
    }
    let tx = a.tree.begin_with_mode(Mode::ReadOnly)?;
    let active: u64 = tx
        .get(b"active:generation")?
        .map(|v| serde_json::from_slice(&v))
        .transpose()?
        .filter(|v| *v > 0)
        .ok_or("retirement requires an active generation")?;
    if active == id {
        return Err("cannot retire the active generation".into());
    }
    let mut base = read_state(&a.tree, id)?;
    if !base.ready {
        return Err("retirement requires a ready generation".into());
    }
    if !state(&a.tree, active)?.ready {
        return Err("retirement requires a ready active generation".into());
    }
    let before = status(a, active)?;
    let at = point(&tx)?;
    let mut names = vec![table_name(id, false)];
    if base.vectors > 0 {
        names.push(table_name(id, true));
    }
    for family in [Family::Text, Family::Vector] {
        let old = applied(&tx, id, family)?;
        if old.pending.is_some() {
            return Err("retirement requires no pending wave".into());
        }
        if old.rows > 0 {
            names.push(family.table(id));
        }
        let current = applied(&tx, active, family)?;
        if current.pending.is_some() || current.watermark != at {
            return Err("retirement requires drained active outbox".into());
        }
    }
    drop(tx);
    if let Some(retirement) = &base.retirement {
        if retirement.deadline_ms != until {
            return Err("original retirement deadline mismatch".into());
        }
        if retirement.completed > names.len() {
            return Err("invalid retirement progress".into());
        }
    } else {
        checkpoint(hold, "before-retire").await;
        // The durable tombstone excludes future readers/builders before any public native deletion.
        base.retirement = Some(Retirement {
            deadline_ms: until,
            completed: 0,
        });
        save(&a.tree, &base).await?;
    }
    let started = Instant::now();
    let ticks = start_ticks()?;
    let completed = base
        .retirement
        .as_ref()
        .ok_or("missing retirement intent")?
        .completed;
    let completed_at_start = completed;
    let progress = |completed: usize| {
        let elapsed = started.elapsed().as_secs_f64();
        let advanced = completed - completed_at_start;
        load::atomic_json(
            &a.root.join(format!("generation-{id}-retire.json")),
            &json!({"phase":"retire_generation","generation":id,"deadline_ms":until,
                "pid":std::process::id(),"start_ticks":ticks,"completed":completed,"total":names.len(),
                "completed_at_start":completed_at_start,"elapsed_s":elapsed,"rate_per_s":advanced as f64/elapsed.max(0.001),
                "eta_s":if completed==names.len(){Some(0.0)}else if advanced==0{None}else{Some((names.len()-completed) as f64*elapsed/advanced as f64)}}),
        )
    };
    progress(completed)?;
    checkpoint(hold, "after-retire-intent").await;
    let connection = connect(a).await?;
    for (index, name) in names.iter().enumerate().skip(completed) {
        remaining(until)?;
        match connection.drop_table(name, &[]).await {
            Ok(()) | Err(lancedb::Error::TableNotFound { .. }) => {}
            Err(error) => return Err(error.into()),
        }
        checkpoint(hold, "after-retire-table").await;
        base.retirement
            .as_mut()
            .ok_or("missing retirement intent")?
            .completed = index + 1;
        save(&a.tree, &base).await?;
        progress(index + 1)?;
    }
    if status(a, active)? != before {
        return Err("retirement changed active authority or watermarks".into());
    }
    Ok(
        json!({"generation":id,"retired":true,"deadline_ms":until,"completed":names.len(),"total":names.len()}),
    )
}

pub(super) fn receipt(a: &Authority, id: u64, request: &str) -> Result<Value, Error> {
    let base = read_state(&a.tree, id)?;
    let receipt = a.stored_receipt(request)?;
    let seq = receipt["commit_seq"]
        .as_u64()
        .ok_or("invalid durable receipt")?;
    let tx = a.tree.begin_with_mode(Mode::ReadOnly)?;
    let mut reasons = serde_json::Map::new();
    let mut watermarks = serde_json::Map::new();
    for family in [Family::Text, Family::Vector] {
        if let Some(c) = completed(&tx, id, family, seq)? {
            reasons.insert(family.name().into(), json!(c.reason));
        }
        watermarks.insert(
            family.name().into(),
            json!(applied(&tx, id, family)?.watermark),
        );
    }
    let mut result = json!({"stage":if base.ready && reasons.len()==2 {"indexed"} else {"committed"},"commit_seq":seq,"generation":id,"bundle":base.bundle,
        "reasons":reasons,"watermarks":watermarks,"visibility":"current_authority_filtered_candidates"});
    if base.retirement.is_some() {
        result["generation_retired"] = json!(true);
    }
    Ok(result)
}
pub(super) async fn activate(a: &Authority, id: u64, hold: Option<&str>) -> Result<Value, Error> {
    if hold.is_some_and(|h| !matches!(h, "before-activation" | "after-activation")) {
        return Err("invalid activation checkpoint".into());
    }
    let _derived = a.derived.lock().await;
    let mut base = state(&a.tree, id)?;
    if !base.ready {
        return Err("candidate generation has incomplete coverage".into());
    }
    super::build(a, &mut base, false).await?;
    for family in [Family::Text, Family::Vector] {
        let p = applied(&a.tree.begin_with_mode(Mode::ReadOnly)?, id, family)?;
        if p.rows > 0 {
            let table = connect(a)
                .await?
                .open_table(family.table(id))
                .execute()
                .await?;
            let name = if family == Family::Text {
                "body_fts"
            } else {
                "vector_ivf"
            };
            for name in ["scope_bitmap", name] {
                let stats = table
                    .index_stats(name)
                    .await?
                    .ok_or("candidate delta index missing")?;
                if stats.num_indexed_rows as u64 != p.rows || stats.num_unindexed_rows != 0 {
                    return Err("candidate delta index incomplete".into());
                }
            }
        }
    }
    let _writer = a.writer.lock().await;
    let mut tx = a.tree.begin()?;
    tx.get_for_update(b"authority:seq")?;
    tx.get_for_update(b"active:generation")?;
    let at = point(&tx)?;
    if !base.ready
        || [Family::Text, Family::Vector]
            .into_iter()
            .map(|f| applied(&tx, id, f))
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .any(|p| p.watermark != at || p.pending.is_some())
    {
        return Err("candidate generation has incomplete coverage".into());
    }
    let prior = tx
        .get(b"active:generation")?
        .map(|v| serde_json::from_slice::<u64>(&v))
        .transpose()?
        .unwrap_or(0);
    if prior == id {
        return Ok(json!({"active":id,"at":at,"reused":true}));
    }
    a.settings.check_disk(&a.root)?;
    tx.set_durability(Durability::Immediate);
    tx.set(b"active:generation", serde_json::to_vec(&id)?)?;
    tx.set(
        format!("activated:{id:020}").as_bytes(),
        serde_json::to_vec(&at)?,
    )?;
    checkpoint(hold, "before-activation").await;
    tx.commit().await?;
    checkpoint(hold, "after-activation").await;
    Ok(json!({"active":id,"previous":prior,"at":at,"bundle":base.bundle,"reused":false}))
}

async fn candidates(
    a: &Authority,
    name: &str,
    text_column: &str,
    scope: u8,
    term: Option<&str>,
    vector: Option<&[f32]>,
) -> Result<Vec<Value>, Error> {
    let table = connect(a).await?.open_table(name).execute().await?;
    let base = table
        .query()
        .select(Select::Columns(vec!["id".into(), "revision".into()]))
        .only_if(format!("scope = {scope}"))
        .limit(20);
    let mut stream = if let Some(vector) = vector {
        base.nearest_to(vector.to_vec())?
            .distance_type(lancedb::DistanceType::Cosine)
            .nprobes(32)
            .execute()
            .await?
    } else {
        base.full_text_search(
            FullTextSearchQuery::new(term.ok_or("missing term")?.into())
                .with_column(text_column.into())?,
        )
        .execute()
        .await?
    };
    let mut hits = Vec::new();
    while let Some(batch) = stream.try_next().await? {
        let ids = column::<UInt64Array>(&batch, "id")?;
        let revisions = column::<UInt64Array>(&batch, "revision")?;
        for row in 0..batch.num_rows() {
            hits.push(json!({"memory":ids.value(row),"seq":revisions.value(row),"partition":format!("p{scope}")}));
        }
    }
    Ok(hits)
}
#[allow(clippy::too_many_arguments)]
pub(super) async fn query_current(
    a: &Authority,
    id: u64,
    bundle: &Bundle,
    principal: &str,
    scope: u8,
    term: Option<String>,
    vector: Option<Vec<f32>>,
    minimum: u64,
) -> Result<Value, Error> {
    let base = state(&a.tree, id)?;
    if !base.ready
        || a.tree
            .begin_with_mode(Mode::ReadOnly)?
            .get(format!("activated:{id:020}").as_bytes())?
            .is_none()
    {
        return Err("query generation unavailable".into());
    }
    if &base.bundle != bundle {
        return Err("query bundle mismatch".into());
    }
    if scope > 7
        || term.is_some() == vector.is_some()
        || term
            .as_ref()
            .is_some_and(|t| t.trim().is_empty() || t.len() > 512)
        || vector.as_ref().is_some_and(|v| {
            v.len() != bundle.dimension
                || v.iter().any(|x| !x.is_finite())
                || v.iter().all(|v| *v == 0.0)
        })
    {
        return Err("invalid search request".into());
    }
    let tx = a.tree.begin_with_mode(Mode::ReadOnly)?;
    let at = point(&tx)?;
    if at < minimum {
        return Err("min_commit_seq_not_reached".into());
    }
    let family = if vector.is_some() {
        Family::Vector
    } else {
        Family::Text
    };
    let progress = applied(&tx, id, family)?;
    if progress.watermark < minimum {
        return Err("index_lag".into());
    }
    drop(tx);
    let mut hits = Vec::new();
    if progress.rows > 0
        || progress
            .pending
            .as_ref()
            .is_some_and(|p| p.appended_version.is_some())
    {
        hits.extend(
            candidates(
                a,
                &family.table(id),
                "body",
                scope,
                term.as_deref(),
                vector.as_deref(),
            )
            .await?,
        );
    }
    if family == Family::Text || base.vectors > 0 {
        hits.extend(
            candidates(
                a,
                &table_name(id, family == Family::Vector),
                &base.text_layout.column(scope),
                scope,
                term.as_deref(),
                vector.as_deref(),
            )
            .await?,
        );
    }
    let mut unique: Vec<Value> = Vec::new();
    for hit in hits {
        if let Some(prior) = unique.iter_mut().find(|old| old["memory"] == hit["memory"]) {
            if prior["seq"].as_u64() < hit["seq"].as_u64() {
                *prior = hit;
            }
        } else {
            unique.push(hit);
        }
    }
    unique.truncate(20);
    let hits = unique;
    let mut result = a.revalidate(principal, scope, at, minimum, &hits).await?;
    result["generation"] = json!(id);
    result["bundle"] = json!(bundle);
    result["watermark"] = json!(progress.watermark);
    result["index_lag"] = json!(progress.watermark < at);
    result["coverage"] = json!("candidate_set_only");
    Ok(result)
}

pub(super) async fn catch_up(a: &Authority, id: u64, waves: u64) -> Result<Value, Error> {
    if !(1..=1_000_000).contains(&waves) {
        return Err("invalid wave count".into());
    }
    if !state(&a.tree, id)?.ready {
        return Err("generation not ready for catch-up".into());
    }
    let key = format!("catchup:{id:020}");
    let mut plan = {
        let _derived = a.derived.lock().await;
        let mut tx = a.tree.begin()?;
        let at = point(&tx)?;
        let saved: Option<CatchUp> = tx
            .get(key.as_bytes())?
            .map(|v| serde_json::from_slice(&v))
            .transpose()?;
        match saved {
            Some(p) if !p.done || p.target == at => p,
            _ => {
                a.settings.check_disk(&a.root)?;
                let p = CatchUp {
                    target: at,
                    deadline_ms: now_ms()? + a.settings.seconds * 1000,
                    done: false,
                };
                tx.set_durability(Durability::Immediate);
                tx.set(key.as_bytes(), serde_json::to_vec(&p)?)?;
                tx.commit().await?;
                p
            }
        }
    };
    let tx = a.tree.begin_with_mode(Mode::ReadOnly)?;
    let initial = applied(&tx, id, Family::Text)?
        .watermark
        .min(applied(&tx, id, Family::Vector)?.watermark);
    drop(tx);
    let started = Instant::now();
    for _ in 0..waves {
        if plan.done {
            break;
        }
        remaining(plan.deadline_ms)?;
        for family in [Family::Text, Family::Vector] {
            let before = applied(&a.tree.begin_with_mode(Mode::ReadOnly)?, id, family)?.watermark;
            if before < plan.target {
                let seqs: Vec<_> = (before + 1..=plan.target.min(before + 256)).collect();
                tokio::time::timeout(
                    remaining(plan.deadline_ms)?,
                    apply(a, id, family, &seqs, None, None, true),
                )
                .await
                .map_err(|_| "original operation deadline expired")??;
            }
        }
        let tx = a.tree.begin_with_mode(Mode::ReadOnly)?;
        let completed = applied(&tx, id, Family::Text)?
            .watermark
            .min(applied(&tx, id, Family::Vector)?.watermark);
        let elapsed = started.elapsed().as_secs_f64();
        let advanced = completed.saturating_sub(initial);
        let rate = advanced as f64 / elapsed;
        load::atomic_json(
            &a.root.join(format!("generation-{id}-catch-up.json")),
            &json!({"phase":"catch_up","completed":completed,"total":plan.target,
            "completed_at_start":initial,"elapsed_s":elapsed,"rate_per_s":rate,"eta_s":if advanced>0 {Some(plan.target.saturating_sub(completed) as f64/rate)} else {None},
            "pid":std::process::id(),"start_ticks":start_ticks()?,"deadline_unix_ms":plan.deadline_ms}),
        )?;
        if completed >= plan.target {
            plan.done = true;
            let mut tx = a.tree.begin()?;
            a.settings.check_disk(&a.root)?;
            tx.set_durability(Durability::Immediate);
            tx.set(key.as_bytes(), serde_json::to_vec(&plan)?)?;
            tx.commit().await?;
        }
    }
    let mut result = status(a, id)?;
    result["target"] = json!(plan.target);
    result["done"] = json!(plan.done);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::Change;
    use tokio::sync::Notify;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn delta_queries_share_session_and_use_scoped_indexes() -> Result<(), Error> {
        let parent = std::env::temp_dir().join(format!(
            "akasha-scope-unit-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        std::fs::create_dir(&parent)?;
        let outcome = async {
            let root = parent.join("database");
            std::fs::write(parent.join("OWNER"), "akasha-otsuge MVP-003 owned corpus import and database\n")?;
            let raw = b"{\"key\":\"baseline\",\"body\":\"baseline\",\"partition\":\"p0\"}\n";
            std::fs::write(parent.join("input.jsonl"), raw)?;
            load::atomic_json(&parent.join("manifest.json"), &json!({"corpus":{"documents":1,"vectors":0,"prepared_bytes":raw.len(),"prepared_sha256":hash(raw),"indexed_body_utf8_bytes":8}}))?;
            load::run(&root, &parent.join("input.jsonl"), &parent.join("manifest.json"), false).await?;
            let a = Authority::open(&root)?;
            let connection = connect(&a).await?;
            let mut plans = Vec::new();
            for family in [Family::Text, Family::Vector] {
                let rows: Vec<_> = (0..8).map(|scope| (1_000_000+scope, load::Document {
                    key: format!("scope:{scope}"), body: "astronomy".into(),
                    partition: format!("p{scope}"), embedding: Some(vec![1.0;1536]),
                })).collect();
                let data = batch(&rows, family == Family::Vector, TextLayout::Legacy)?;
                let table = connection.create_table(family.table(1), data).execute().await?;
                let reader = connect(&a).await?.open_table(family.table(1)).execute().await?;
                assert_eq!(reader.count_rows(None).await?, 8);
                let writer_session = table.dataset().unwrap().get().await?.session();
                let reader_session = reader.dataset().unwrap().get().await?.session();
                let shared = Arc::ptr_eq(&writer_session, &reader_session);
                let extra = batch(&[(1_000_008, load::Document {
                    key: "fresh".into(), body: "fresh".into(), partition: "p7".into(),
                    embedding: Some(vec![1.0;1536]),
                })], family == Family::Vector, TextLayout::Legacy)?;
                table.add(extra).write_parallelism(1).execute().await?;
                refresh(&table, &a, family, 9, now_ms()?+60_000).await?;
                let fresh = connect(&a).await?.open_table(family.table(1)).execute().await?;
                assert_eq!(fresh.count_rows(None).await?, 9);
                let fresh_hits = candidates(&a, &family.table(1), "body", 7,
                    (family == Family::Text).then_some("fresh"),
                    (family == Family::Vector).then_some(vec![1.0;1536]).as_deref()).await?;
                assert!(fresh_hits.iter().any(|hit| hit["memory"] == 1_000_008));
                let query = table.query().only_if("scope = 3").limit(20);
                let plan = if family == Family::Text {
                    query.full_text_search(FullTextSearchQuery::new("astronomy".into()).with_column("body".into())?).explain_plan(false).await?
                } else {
                    query.nearest_to(vec![1.0;1536])?.distance_type(lancedb::DistanceType::Cosine).explain_plan(false).await?
                };
                let hits = candidates(&a, &family.table(1), "body", 3,
                    (family == Family::Text).then_some("astronomy"),
                    (family == Family::Vector).then_some(vec![1.0;1536]).as_deref()).await?;
                plans.push((plan, hits, shared));
            }
            a.tree.close().await?;
            Ok::<_,Error>(plans)
        }.await;
        std::fs::remove_dir_all(&parent)?;
        for (plan, hits, shared) in outcome? {
            assert!(
                shared,
                "connections for one authority must share the Lance cache and spill budget"
            );
            assert!(
                plan.contains("ScalarIndexQuery") && plan.contains("scope_bitmap(Bitmap)"),
                "{plan}"
            );
            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0]["memory"], 1_000_003);
            assert_eq!(hits[0]["partition"], "p3");
        }
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn indexing_releases_authority_writer_and_filters_replaced_head() -> Result<(), Error> {
        let parent = std::env::temp_dir().join(format!(
            "akasha-outbox-unit-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        std::fs::create_dir(&parent)?;
        let outcome=async {
            let root=parent.join("database");
            std::fs::write(parent.join("OWNER"),"akasha-otsuge MVP-003 owned corpus import and database\n")?;
            let raw=b"{\"key\":\"baseline\",\"body\":\"baseline\",\"partition\":\"p0\"}\n";
            std::fs::write(parent.join("input.jsonl"),raw)?;
            load::atomic_json(&parent.join("manifest.json"),&json!({"corpus":{"documents":1,"vectors":0,"prepared_bytes":raw.len(),"prepared_sha256":hash(raw),"indexed_body_utf8_bytes":8}}))?;
            load::run(&root,&parent.join("input.jsonl"),&parent.join("manifest.json"),false).await?;
            let bundle=json!({"model":"unit","model_fingerprint":"a".repeat(64),"dimension":1536,"chunker":"unit","schema":1});
            for request in [json!({"op":"begin","generation":1,"bundle":bundle}),json!({"op":"backfill","generation":1,"waves":1}),json!({"op":"build","generation":1})] {
                load::atomic_json(&parent.join("request.json"),&request)?;
                super::super::run(&root,&parent.join("request.json")).await?;
            }
            let mut a=Authority::open(&root)?;
            let entered=Arc::new(Notify::new());let resume=Arc::new(Notify::new());
            a.derived_barrier=Some((entered.clone(),resume.clone()));
            let a=Arc::new(a);
            let initial=Change{request_id:"one".into(),memory:1,expected:0,scope:0,active:true,body:"old astronomy".into(),text_only:true};
            a.apply(&initial,None).await?;a.set_permission("reader",0,true).await?;
            let worker=a.clone();
            let task=tokio::spawn(async move {apply(&worker,1,Family::Text,&[1],None,None,false).await});
            let reached=tokio::time::timeout(Duration::from_secs(30),entered.notified()).await;
            let second=Change{request_id:"two".into(),expected:1,body:"new astronomy".into(),..initial};
            let written=tokio::time::timeout(Duration::from_secs(5),a.apply(&second,None)).await;
            resume.notify_one();let indexed=task.await;
            let at=point(&a.tree.begin_with_mode(Mode::ReadOnly)?)?;
            let checked=a.revalidate("reader",0,1,1,&[json!({"memory":1,"seq":1,"partition":"p0"})]).await;
            let a=Arc::try_unwrap(a).map_err(|_|"derived test retained authority")?;a.tree.close().await?;
            reached?;assert_eq!(written??["commit_seq"],2);indexed??;
            assert_eq!(at,2);assert_eq!(checked?["hits"],json!([]));
            Ok::<_,Error>(())
        }.await;
        std::fs::remove_dir_all(&parent)?;
        outcome
    }
}
