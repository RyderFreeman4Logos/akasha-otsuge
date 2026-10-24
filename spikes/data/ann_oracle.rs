//! Bounded exact cosine ground truth over the pinned public vector stream.
use std::{
    fs::{self, File},
    io::{self, BufReader, Read, Write},
    path::Path,
    time::Instant,
};

struct Record {
    key: String,
    partition: u8,
    vector: Vec<f64>,
    norm: f64,
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid vector oracle input")
}

fn count(reader: &mut impl Read) -> io::Result<u32> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn record(reader: &mut impl Read) -> io::Result<Record> {
    let mut header = [0; 3];
    reader.read_exact(&mut header)?;
    let length = u16::from_le_bytes([header[1], header[2]]) as usize;
    if header[0] > 7 || !(1..=256).contains(&length) {
        return Err(invalid());
    }
    let mut key = vec![0; length];
    reader.read_exact(&mut key)?;
    let mut bytes = [0; 1536 * 4];
    reader.read_exact(&mut bytes)?;
    let vector: Vec<f64> = bytes
        .chunks_exact(4)
        .map(|v| f32::from_le_bytes(v.try_into().unwrap()) as f64)
        .collect();
    let norm = vector.iter().map(|v| v * v).sum::<f64>().sqrt();
    if !norm.is_finite() || norm == 0.0 {
        return Err(invalid());
    }
    Ok(Record {
        key: String::from_utf8(key).map_err(|_| invalid())?,
        partition: header[0],
        vector,
        norm,
    })
}

fn cosine(left: &Record, right: &Record) -> f64 {
    let mut sums = [0.0; 4];
    for (a, b) in left
        .vector
        .chunks_exact(4)
        .zip(right.vector.chunks_exact(4))
    {
        for i in 0..4 {
            sums[i] += a[i] * b[i];
        }
    }
    sums.iter().sum::<f64>() / (left.norm * right.norm)
}

fn progress(
    path: &Path,
    completed: u64,
    total: u64,
    started: Instant,
    unit: usize,
) -> io::Result<()> {
    let elapsed = started.elapsed().as_secs_f64();
    let stat = fs::read_to_string("/proc/self/stat")?;
    let ticks = stat
        .rsplit_once(')')
        .and_then(|(_, s)| s.split_whitespace().nth(19))
        .ok_or_else(invalid)?;
    let temporary = path.with_extension("tmp");
    let mut file = File::create(&temporary)?;
    writeln!(
        file,
        "{{\"phase\":\"exact_cosine\",\"completed\":{completed},\"total\":{total},\"elapsed_s\":{elapsed},\"rate_per_s\":{},\"eta_s\":{},\"active_unit\":{unit},\"pid\":{},\"start_ticks\":\"{ticks}\"}}",
        completed as f64 / elapsed,
        if completed == 0 {
            "null".into()
        } else {
            ((total - completed) as f64 * elapsed / completed as f64).to_string()
        },
        std::process::id()
    )?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    File::open(path.parent().ok_or_else(invalid)?)?.sync_all()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 6 {
        return Err(
            "usage: ann-oracle QUERY_BIN OWNED_CORPUS_DIR SHARDS EXPECTED_ROWS PROGRESS_JSON"
                .into(),
        );
    }
    let shards: usize = args[3].to_str().ok_or_else(invalid)?.parse()?;
    let expected: u64 = args[4].to_str().ok_or_else(invalid)?.parse()?;
    if !(1..=26).contains(&shards) || expected == 0 || expected > 1_000_000 {
        return Err(invalid().into());
    }
    let mut query_reader = BufReader::new(File::open(&args[1])?);
    let n = count(&mut query_reader)?;
    if n == 0 || n > 64 {
        return Err(invalid().into());
    }
    let queries: Vec<_> = (0..n)
        .map(|_| record(&mut query_reader))
        .collect::<io::Result<_>>()?;
    if query_reader.read(&mut [0])? != 0 {
        return Err(invalid().into());
    }
    let mut top = vec![Vec::<(f64, String)>::new(); queries.len()];
    let started = Instant::now();
    let mut completed = 0;
    for unit in 0..shards {
        let mut reader = BufReader::with_capacity(
            1024 * 1024,
            File::open(Path::new(&args[2]).join(format!("unit-{unit}.bin")))?,
        );
        let rows = count(&mut reader)?;
        if rows == 0 || completed + u64::from(rows) > expected {
            return Err(invalid().into());
        }
        for _ in 0..rows {
            let value = record(&mut reader)?;
            for (query, neighbors) in queries.iter().zip(&mut top) {
                if query.partition != value.partition {
                    continue;
                }
                let score = cosine(query, &value);
                if neighbors.len() < 20 || score >= neighbors.last().unwrap().0 {
                    neighbors.push((score, value.key.clone()));
                    neighbors.sort_unstable_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
                    neighbors.truncate(20);
                }
            }
            completed += 1;
            if completed % 16384 == 0 {
                progress(Path::new(&args[5]), completed, expected, started, unit)?;
            }
        }
        if reader.read(&mut [0])? != 0 {
            return Err(invalid().into());
        }
    }
    if completed != expected || top.iter().any(|rows| rows.len() != 20) {
        return Err(invalid().into());
    }
    progress(Path::new(&args[5]), completed, expected, started, shards)?;
    for (query, neighbors) in top.iter().enumerate() {
        for (rank, (score, key)) in neighbors.iter().enumerate() {
            let hex: String = key.bytes().map(|b| format!("{b:02x}")).collect();
            println!("{query}\t{rank}\t{hex}\t{score:.17}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cosine_has_known_geometry() {
        let make = |x: f64, y: f64| {
            let mut vector = vec![0.0; 1536];
            vector[0] = x;
            vector[1] = y;
            Record {
                key: "x".into(),
                partition: 0,
                norm: (x * x + y * y).sqrt(),
                vector,
            }
        };
        assert!((cosine(&make(3.0, 4.0), &make(6.0, 8.0)) - 1.0).abs() < 1e-14);
        assert_eq!(cosine(&make(1.0, 0.0), &make(0.0, 1.0)), 0.0);
        assert_eq!(cosine(&make(1.0, 0.0), &make(-1.0, 0.0)), -1.0);
    }
    #[test]
    fn malformed_records_are_rejected() {
        assert!(record(&mut [8, 1, 0].as_slice()).is_err());
        assert!(record(&mut [0, 0, 0].as_slice()).is_err());
        let mut zero = vec![0, 1, 0, b'x'];
        zero.resize(4 + 1536 * 4, 0);
        assert!(record(&mut zero.as_slice()).is_err());
    }
}
