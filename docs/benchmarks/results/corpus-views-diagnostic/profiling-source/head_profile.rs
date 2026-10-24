use std::{path::Path,time::Instant};
use helix_db::prelude::*;
use serde_json::{json,Value};
use crate::{authority::Authority,engine::Error};

pub async fn run(a:&Authority,path:&Path)->Result<Value,Error> {
    let input:Value=serde_json::from_slice(&std::fs::read(path)?)?;
    let memories:Vec<u64>=input["queries"].as_array().ok_or("queries")?.iter().take(20).map(|r|r["memory"].as_u64().ok_or("memory")).collect::<Result<_,_>>()?;
    if memories.len()!=20 {return Err("20 required".into());}
    let at=a.read_point().await?;let mut samples=Vec::new();
    for repeat in 0..5 {
        for mode in ["combined","heads","revisions","sources","import"] {
            let started=Instant::now();
            if mode=="combined" {let _=a.checked_heads(&memories,at).await?;} else {
                let mut batch=read_batch();let mut names=Vec::new();
                for (i,memory) in memories.iter().enumerate() {
                    if mode=="import" && i>0 {break;}
                    let name=format!("row{i}");
                    let traversal=match mode {
                        "heads"=>g().n_with_label_where("Head",SourcePredicate::eq("key",i64::try_from(*memory)?)).limit(2).value_map(Some(vec!["seq","scope","active"])),
                        "revisions"=>g().n_with_label_where("Revision",SourcePredicate::between("order",format!("{memory:020}:00000000000000000000"),format!("{memory:020}:{:020}",i64::MAX))).order_by("order",Order::Desc).limit(1).value_map(Some(vec!["memory","seq","scope","active"])),
                        "sources"=>g().n_with_label_where("Document",SourcePredicate::eq("memory",i64::try_from(*memory)?)).limit(2).value_map(Some(vec!["memory","key","partition"])),
                        _=>g().n_with_label("ImportState").limit(2).value_map(Some(vec!["documents","complete"])),
                    };
                    batch=batch.var_as(&name,traversal);names.push(name);
                }
                let _=a.engine.query(QueryRequest::read(batch.returning(names))).await?;
            }
            samples.push(json!({"repeat":repeat,"mode":mode,"seconds":started.elapsed().as_secs_f64(),"memories":memories.len()}));
        }
    }
    Ok(json!({"samples":samples,"diagnostic_only":true}))
}
