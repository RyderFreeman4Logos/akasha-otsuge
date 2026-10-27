use std::{sync::atomic::{AtomicU64,Ordering::Relaxed},time::Instant};
const LABELS:[&str;10]=["native","receipt","head","counter","membership","commit","other_cypher","head_batch","current","historical_page"];
static COUNTS:[AtomicU64;10]=[const {AtomicU64::new(0)};10];
static TOTAL:[AtomicU64;10]=[const {AtomicU64::new(0)};10];
static MAX:[AtomicU64;10]=[const {AtomicU64::new(0)};10];
pub struct Timer(usize,Instant);
pub fn start(n:usize)->Timer{Timer(n,Instant::now())}
impl Drop for Timer{fn drop(&mut self){let ns=self.1.elapsed().as_nanos() as u64;COUNTS[self.0].fetch_add(1,Relaxed);TOTAL[self.0].fetch_add(ns,Relaxed);MAX[self.0].fetch_max(ns,Relaxed);}}
pub fn reset(){for values in [&COUNTS,&TOTAL,&MAX]{for v in values{v.store(0,Relaxed);}}}
pub fn dump(){let v:Vec<_>=(0..10).map(|i|serde_json::json!({"kind":LABELS[i],"count":COUNTS[i].load(Relaxed),"total_s":TOTAL[i].load(Relaxed) as f64/1e9,"max_s":MAX[i].load(Relaxed) as f64/1e9})).collect();eprintln!("QUERY_PROFILE:{}",serde_json::json!({"inclusive_spans":v,"diagnostic_only":true}));}
