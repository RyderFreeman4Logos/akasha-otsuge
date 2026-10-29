use super::*;
use crate::domain::{RecallStatus, SourceSpan};
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResearchMemory {
    pub memory_id: String,
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResearchConclusion {
    pub status: RecallStatus,
    pub answer: String,
    pub sources: Vec<SourceSpan>,
    pub memories: Vec<ResearchMemory>,
    pub conditions: Vec<String>,
    pub conflicts: Vec<String>,
    pub gaps: Vec<String>,
}

impl ResearchConclusion {
    pub fn parse(text: &str) -> Result<Self> {
        let result: Self = serde_json::from_str(text).map_err(|_| ErrorCode::ProtocolInvalid)?;
        if result.answer.trim().is_empty()
            || result.answer.len() > MAX_FRAME_BYTES
            || result.sources.len() > 64
            || result.memories.len() > 32
            || result.conditions.len() > 32
            || result.conflicts.len() > 32
            || result.gaps.len() > 32
            || result
                .conditions
                .iter()
                .chain(&result.conflicts)
                .any(|s| s.trim().is_empty() || s.len() > 4096)
            || result.gaps.iter().any(|s| !valid_id(s))
            || result
                .memories
                .iter()
                .any(|m| !valid_id(&m.memory_id) || m.revision == 0)
            || result.status == RecallStatus::Found
                && result.sources.is_empty()
                && result.memories.is_empty()
            || result.status == RecallStatus::NoHit
                && (!result.sources.is_empty() || !result.memories.is_empty())
            || result.status == RecallStatus::Incomplete
                && result.gaps.is_empty()
                && result.conflicts.is_empty()
        {
            return Err(ErrorCode::ProtocolInvalid.into());
        }
        Ok(result)
    }

    pub fn schema() -> Value {
        let source = json!({"type":"object","properties":{
            "event_id":{"type":"string"},"revision":{"type":"integer","minimum":1},
            "content_sha256":{"type":"string"},"start":{"type":"integer","minimum":0},
            "end":{"type":"integer","minimum":1}},
            "required":["event_id","revision","content_sha256","start","end"],"additionalProperties":false});
        json!({"type":"object","properties":{
            "status":{"type":"string","enum":["found","no_hit","incomplete"]},
            "answer":{"type":"string"},"sources":{"type":"array","items":source,"maxItems":64},
            "memories":{"type":"array","maxItems":32,"items":{"type":"object","properties":{
                "memory_id":{"type":"string"},"revision":{"type":"integer","minimum":1}},
                "required":["memory_id","revision"],"additionalProperties":false}},
            "conditions":{"type":"array","items":{"type":"string"},"maxItems":32},
            "conflicts":{"type":"array","items":{"type":"string"},"maxItems":32},
            "gaps":{"type":"array","items":{"type":"string"},"maxItems":32}},
            "required":["status","answer","sources","memories","conditions","conflicts","gaps"],"additionalProperties":false})
    }
}
