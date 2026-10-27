// Frozen oracle version 1, shared by both specified storage candidates.
pub const ORACLE_MEMORIES: u64 = 1024;

pub fn oracle_change(seq: u64) -> Change {
    let memory = (seq - 1) % ORACLE_MEMORIES;
    let round = (seq - 1) / ORACLE_MEMORIES;
    Change {
        request_id: format!("oracle-v1:{seq}"),
        memory,
        expected: seq.saturating_sub(ORACLE_MEMORIES),
        scope: ((memory + round) % 8) as u8,
        active: round % 4 != 1,
        text_only: false,
        body: format!(
            "Synthetic oracle only; excluded from corpus bytes. Memory {memory}, revision round {round}."
        ),
    }
}

pub fn oracle_state(memory: u64, at: u64) -> Option<Value> {
    if at <= memory {
        return None;
    }
    let seq = memory + 1 + ((at - memory - 1) / ORACLE_MEMORIES) * ORACLE_MEMORIES;
    let change = oracle_change(seq);
    Some(
        json!({"memory":memory,"seq":seq,"scope":change.scope,"active":change.active,"body":change.body}),
    )
}
