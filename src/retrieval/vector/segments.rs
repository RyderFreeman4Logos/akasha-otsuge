use super::*;
use arrow_array::Float32Array;
use lance::{
    Dataset,
    dataset::scanner::Scanner,
    index::{DatasetIndexExt, DatasetIndexInternalExt},
};

const MAX_SEGMENTS: usize = 4;
const MAX_FILE_BYTES: u64 = 8 << 20;

pub(in crate::retrieval) async fn search(
    dataset: &Dataset,
    query: &[f32],
    filter: &str,
    limit: usize,
) -> Result<Option<Vec<(String, u64)>>> {
    let column = dataset.schema().field_id("vector").map_err(index_error)? as u32;
    let indices = dataset.describe_indices(None).await.map_err(index_error)?;
    let mut matching = indices.iter().filter(|index| index.field_ids() == [column]);
    let Some(index) = matching.next() else {
        return Ok(None);
    };
    if matching.next().is_some() || !index.index_type().eq_ignore_ascii_case("IVF_FLAT") {
        return Ok(None);
    }
    let segments = index.segments();
    if segments.len() < 2 || segments.iter().any(|s| s.fragment_bitmap.is_none()) {
        return Ok(None);
    }
    // An oversized or unmeasured segment runs alone. The byte limit describes stored index
    // files; vector decoding, the query engine and its result buffers have separate costs.
    let query = Float32Array::from(query.to_vec());
    let scanner = || -> Result<Scanner> {
        let mut scan = dataset.scan();
        scan.nearest("vector", &query, limit)
            .map_err(index_error)?
            .distance_metric(DistanceType::Cosine.into())
            .nprobes(16)
            .prefilter(true)
            .with_row_id();
        scan.filter(filter).map_err(index_error)?;
        scan.project(&["_distance", "_rowid"])
            .map_err(index_error)?;
        Ok(scan)
    };
    let mut best = Vec::new();
    let mut rest = segments;
    while !rest.is_empty() {
        let count = batch_len(rest.iter().map(|segment| {
            segment.files.as_ref().and_then(|files| {
                (!files.is_empty()).then(|| {
                    files
                        .iter()
                        .fold(0u64, |n, file| n.saturating_add(file.size_bytes))
                })
            })
        }));
        // The ordinary query already runs one bounded native group and covers its unindexed tail.
        if count == segments.len() {
            return Ok(None);
        }
        let (group, next) = rest.split_at(count);
        let mut scanner = scanner()?;
        scanner
            .with_index_segments(group.iter().map(|s| s.uuid).collect())
            .map_err(index_error)?;
        collect(&mut scanner, limit, &mut best).await?;
        rest = next;
    }
    let unindexed = dataset
        .unindexed_fragments(index.name())
        .await
        .map_err(index_error)?;
    let mut rest = unindexed.as_slice();
    while !rest.is_empty() {
        let count = batch_len(rest.iter().map(|fragment| {
            fragment
                .physical_rows
                .map(|rows| (rows as u64).saturating_mul(query.len() as u64 * 4 + 32))
        }));
        let (group, next) = rest.split_at(count);
        let mut scanner = scanner()?;
        scanner.with_fragments(group.to_vec()).use_index(false);
        collect(&mut scanner, limit, &mut best).await?;
        rest = next;
    }
    if best.is_empty() {
        return Ok(Some(Vec::new()));
    }
    let row_ids: Vec<_> = best.into_iter().map(|(_, id)| id).collect();
    let projection =
        lance::dataset::ProjectionRequest::from_columns(["id", "revision"], dataset.schema());
    let projected = dataset
        .take_rows(&row_ids, projection)
        .await
        .map_err(index_error)?;
    let ids = projected
        .column_by_name("id")
        .and_then(|a| a.as_any().downcast_ref::<StringArray>())
        .ok_or_else(|| error("INDEX_CORRUPT"))?;
    let revisions = projected
        .column_by_name("revision")
        .and_then(|a| a.as_any().downcast_ref::<UInt64Array>())
        .ok_or_else(|| error("INDEX_CORRUPT"))?;
    if projected.num_rows() != row_ids.len() || ids.null_count() != 0 || revisions.null_count() != 0
    {
        return Err(error("INDEX_CORRUPT"));
    }
    Ok(Some(
        (0..ids.len())
            .map(|i| (ids.value(i).to_owned(), revisions.value(i)))
            .collect(),
    ))
}

fn batch_len(sizes: impl Iterator<Item = Option<u64>>) -> usize {
    let mut bytes = 0u64;
    let mut count = 0;
    for size in sizes.take(MAX_SEGMENTS) {
        let size = size.filter(|&s| s != 0).unwrap_or(u64::MAX);
        if count > 0 && bytes.saturating_add(size) > MAX_FILE_BYTES {
            break;
        }
        bytes = bytes.saturating_add(size);
        count += 1;
    }
    count
}

async fn collect(scan: &mut Scanner, limit: usize, best: &mut Vec<(f32, u64)>) -> Result<()> {
    let mut stream = scan.try_into_stream().await.map_err(index_error)?;
    while let Some(batch) = stream.try_next().await.map_err(index_error)? {
        let dist = batch
            .column_by_name("_distance")
            .and_then(|a| a.as_any().downcast_ref::<Float32Array>())
            .ok_or_else(|| error("INDEX_CORRUPT"))?;
        let rowids = batch
            .column_by_name("_rowid")
            .and_then(|a| a.as_any().downcast_ref::<UInt64Array>())
            .ok_or_else(|| error("INDEX_CORRUPT"))?;
        if dist.null_count() != 0 || rowids.null_count() != 0 {
            return Err(error("INDEX_CORRUPT"));
        }
        for i in 0..batch.num_rows() {
            let d = dist.value(i);
            if !d.is_finite() {
                return Err(error("INDEX_CORRUPT"));
            }
            best.push((d, rowids.value(i)));
        }
    }
    // This is the same global distance/row-id tiebreak as the native ANN TopK.
    best.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut seen = std::collections::HashSet::with_capacity(best.len());
    best.retain(|row| seen.insert(row.1));
    best.truncate(limit);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_bound_count_and_known_bytes_and_isolate_unknown_large_files() {
        assert_eq!(batch_len([Some(1); 9].into_iter()), MAX_SEGMENTS);
        assert_eq!(batch_len([Some(MAX_FILE_BYTES / 2); 4].into_iter()), 2);
        assert_eq!(
            batch_len([Some(MAX_FILE_BYTES + 1), Some(1)].into_iter()),
            1
        );
        assert_eq!(batch_len([Some(1), None, Some(1)].into_iter()), 1);
        assert_eq!(batch_len([None, Some(1)].into_iter()), 1);
    }
}
