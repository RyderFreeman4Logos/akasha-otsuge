-- Structural fixture from a real schema21/fork28 snapshot; no source rows.
-- Supported source mapping pin: 4593a5c5e6a9ea1140256cd42fa5838511a24e14.
-- This fixture does not attest the installed source binary.
PRAGMA user_version=21;
CREATE TABLE consolidation_log (
    id TEXT PRIMARY KEY,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    wing TEXT NOT NULL,
    room TEXT,
    project_id TEXT,
    cluster_size INTEGER NOT NULL,
    strategy TEXT NOT NULL CHECK(strategy IN ('richest_content', 'llm_summary')),
    target_drawer_id TEXT NOT NULL REFERENCES drawers(id),
    source_drawer_ids TEXT NOT NULL,
    dry_run INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE conversation_turns (
    id              TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL,
    tool            TEXT NOT NULL,
    turn_index      INTEGER NOT NULL,
    role            TEXT NOT NULL,
    content         TEXT NOT NULL,
    timestamp_epoch REAL NOT NULL,
    token_count     INTEGER,
    project_path    TEXT,
    git_branch      TEXT,
    is_csa_delegated BOOLEAN NOT NULL DEFAULT FALSE,
    provenance       TEXT NOT NULL DEFAULT 'human', hermes_profile TEXT, session_title TEXT, session_source TEXT, message_id TEXT, tool_name TEXT, tool_call_id TEXT, previous_message_id TEXT, next_message_id TEXT,
    UNIQUE(session_id, tool, turn_index)
);
CREATE TABLE design_insights (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL CHECK(source IN ('user_idea', 'review_finding', 'tool_friction', 'incident', 'research')),
    scope TEXT NOT NULL CHECK(scope IN ('project', 'cross_project', 'repo', 'issue')),
    target_artifact TEXT NOT NULL CHECK(target_artifact IN ('memory', 'skill', 'agents_rule', 'agents_rules_ref', 'codex_skill', 'github_issue', 'mempal_knowledge')),
    evidence_ref TEXT NOT NULL,
    summary TEXT NOT NULL,
    rule_text TEXT,
    priority INTEGER NOT NULL CHECK(priority BETWEEN 1 AND 5),
    status TEXT NOT NULL CHECK(status IN ('open', 'resolved')) DEFAULT 'open',
    created_at TEXT NOT NULL,
    resolved_at TEXT,
    resolved_by TEXT,
    resolution_note TEXT,
    redaction_count INTEGER NOT NULL DEFAULT 0,
    project_id TEXT
);
CREATE TABLE drawers (
    id TEXT PRIMARY KEY,
    content TEXT NOT NULL,
    wing TEXT NOT NULL,
    room TEXT,
    source_file TEXT,
    source_type TEXT NOT NULL DEFAULT 'system_generated' CHECK(source_type IN ('user_explicit', 'agent_observation', 'agent_inference', 'system_generated')),
    added_at TEXT NOT NULL,
    chunk_index INTEGER
, deleted_at TEXT, importance INTEGER DEFAULT 0, merge_count INTEGER NOT NULL DEFAULT 0, updated_at TEXT, project_id TEXT, content_hash TEXT, normalize_version INTEGER NOT NULL DEFAULT 1, memory_kind TEXT NOT NULL DEFAULT 'evidence' CHECK(memory_kind IN ('evidence', 'knowledge', 'atomic_fact', 'decision', 'case', 'skill', 'foresight', 'profile_fact', 'profile_trait')), domain TEXT NOT NULL DEFAULT 'project' CHECK(domain IN ('user', 'agent', 'project', 'skill', 'global')), field TEXT, anchor_kind TEXT NOT NULL CHECK(anchor_kind IN ('global', 'repo', 'worktree')) DEFAULT 'repo', anchor_id TEXT NOT NULL DEFAULT 'repo://legacy', parent_anchor_id TEXT, provenance TEXT CHECK(provenance IN ('runtime', 'research', 'human')), statement TEXT, tier TEXT CHECK(tier IN ('qi', 'shu', 'dao_ren', 'dao_tian')), status TEXT CHECK(status IN ('active', 'superseded', 'pending_review', 'candidate', 'promoted', 'canonical', 'demoted', 'retired')), supporting_refs TEXT NOT NULL DEFAULT '[]', counterexample_refs TEXT NOT NULL DEFAULT '[]', teaching_refs TEXT NOT NULL DEFAULT '[]', verification_refs TEXT NOT NULL DEFAULT '[]', scope_constraints TEXT, trigger_hints TEXT, last_accessed_at INTEGER, access_count INTEGER NOT NULL DEFAULT 0, accumulated_boost REAL NOT NULL DEFAULT 0.0, effective_importance REAL NOT NULL DEFAULT 0.0, stale_penalty_applied REAL NOT NULL DEFAULT 1.0, valid_from TEXT, valid_until TEXT, confidence REAL NOT NULL DEFAULT 0.5, compacted_into TEXT REFERENCES drawers(id), is_pinned INTEGER NOT NULL DEFAULT 0 CHECK(is_pinned IN (0, 1)), pin_order INTEGER, supersedes TEXT REFERENCES drawers(id), consolidation_priority REAL, last_sleep_at TEXT, source_root TEXT, admission_owner TEXT, creation_operation_id TEXT);
CREATE TABLE embed_failure_log (
    id TEXT PRIMARY KEY,
    timestamp INTEGER NOT NULL,
    error_message TEXT NOT NULL,
    endpoint TEXT,
    consecutive_failures INTEGER,
    duration_ms INTEGER,
    retained_until INTEGER NOT NULL
);
CREATE TABLE failure_events (
    event_id      TEXT PRIMARY KEY,
    drawer_id     TEXT NOT NULL,
    wing          TEXT NOT NULL,
    room          TEXT,
    topic_sig     TEXT NOT NULL,
    failure_type  TEXT NOT NULL,
    detected_at   INTEGER NOT NULL,
    project_id    TEXT
);
CREATE TABLE foresights (
    drawer_id TEXT PRIMARY KEY REFERENCES drawers(id) ON DELETE CASCADE,
    statement TEXT NOT NULL,
    reason TEXT,
    trigger_condition TEXT NOT NULL,
    due_at TEXT NOT NULL,
    valid_from TEXT,
    valid_until TEXT,
    resolved_at TEXT,
    resolution_note TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE fork_ext_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE gating_audit (
    id TEXT PRIMARY KEY,
    candidate_hash TEXT NOT NULL,
    drawer_id TEXT,
    decision TEXT NOT NULL CHECK(decision IN ('keep', 'skip')),
    tier INTEGER NOT NULL,
    label TEXT,
    reason TEXT,
    score REAL,
    explain_json TEXT NOT NULL,
    retained_until INTEGER NOT NULL,
    created_at INTEGER NOT NULL
, project_id TEXT, llm_verdict TEXT, llm_score REAL, content_preview TEXT);
CREATE TABLE historical_rejudge_work_items (
            run_id TEXT NOT NULL,
            drawer_rowid INTEGER NOT NULL,
            drawer_id TEXT NOT NULL,
            snapshot_content_hash TEXT NOT NULL,
            snapshot_added_at TEXT NOT NULL,
            snapshot_wing TEXT NOT NULL,
            snapshot_room TEXT,
            snapshot_source_file TEXT,
            snapshot_source_type TEXT NOT NULL,
            snapshot_project_id TEXT,
            snapshot_chunk_index INTEGER,
            snapshot_normalize_version INTEGER NOT NULL,
            processed_at TEXT,
            decision TEXT,
            mutation TEXT, llm_stage TEXT, llm_proposal_score REAL,
            PRIMARY KEY (run_id, drawer_rowid)
        );
CREATE TABLE knowledge_cards (
    id TEXT PRIMARY KEY,
    statement TEXT NOT NULL,
    content TEXT NOT NULL,
    tier TEXT NOT NULL CHECK(tier IN ('qi', 'shu', 'dao_ren', 'dao_tian')),
    status TEXT NOT NULL CHECK(status IN ('pending_review', 'candidate', 'promoted', 'canonical', 'demoted', 'retired')),
    domain TEXT NOT NULL CHECK(domain IN ('project', 'agent', 'skill', 'global')),
    field TEXT NOT NULL DEFAULT 'general',
    anchor_kind TEXT NOT NULL CHECK(anchor_kind IN ('global', 'repo', 'worktree')),
    anchor_id TEXT NOT NULL,
    parent_anchor_id TEXT,
    scope_constraints TEXT,
    trigger_hints TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
, auto_generated INTEGER NOT NULL DEFAULT 0 CHECK(auto_generated IN (0, 1)), crystallization_score REAL, source_drawer_ids TEXT NOT NULL DEFAULT '[]');
CREATE TABLE knowledge_events (
    id TEXT PRIMARY KEY,
    card_id TEXT NOT NULL,
    event_type TEXT NOT NULL CHECK(event_type IN ('created', 'promoted', 'demoted', 'retired', 'linked', 'unlinked', 'updated', 'published_anchor')),
    from_status TEXT,
    to_status TEXT,
    reason TEXT NOT NULL,
    actor TEXT,
    metadata TEXT,
    created_at TEXT NOT NULL,
    FOREIGN KEY(card_id) REFERENCES knowledge_cards(id) ON DELETE RESTRICT
);
CREATE TABLE knowledge_evidence_links (
    id TEXT PRIMARY KEY,
    card_id TEXT NOT NULL,
    evidence_drawer_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK(role IN ('supporting', 'verification', 'counterexample', 'teaching')),
    note TEXT,
    created_at TEXT NOT NULL,
    UNIQUE(card_id, evidence_drawer_id, role),
    FOREIGN KEY(card_id) REFERENCES knowledge_cards(id) ON DELETE RESTRICT,
    FOREIGN KEY(evidence_drawer_id) REFERENCES drawers(id) ON DELETE RESTRICT
);
CREATE TABLE leases (
    resource_path TEXT NOT NULL PRIMARY KEY,
    holder_id TEXT NOT NULL,
    acquired_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    expires_at TEXT NOT NULL,
    metadata TEXT
);
CREATE TABLE novelty_audit (
    id TEXT PRIMARY KEY,
    candidate_hash TEXT NOT NULL,
    decision TEXT NOT NULL,
    near_drawer_id TEXT,
    cosine REAL,
    created_at INTEGER NOT NULL
, project_id TEXT);
CREATE TABLE operation_telemetry (
    id                         TEXT PRIMARY KEY,
    started_at_unix_ms         INTEGER NOT NULL,
    duration_ms                INTEGER NOT NULL,
    source                     TEXT NOT NULL CHECK(source IN ('cli', 'mcp', 'rest', 'daemon')),
    operation                  TEXT NOT NULL,
    call_site                  TEXT NOT NULL,
    success                    INTEGER NOT NULL CHECK(success IN (0, 1)),
    error_class                TEXT,
    sqlite_error_class         TEXT,
    rows_scanned               INTEGER NOT NULL DEFAULT 0,
    rows_returned              INTEGER NOT NULL DEFAULT 0,
    rows_changed               INTEGER NOT NULL DEFAULT 0,
    lock_wait_count            INTEGER NOT NULL DEFAULT 0,
    retry_count                INTEGER NOT NULL DEFAULT 0,
    result_count               INTEGER NOT NULL DEFAULT 0,
    physical_read_bytes        INTEGER NOT NULL DEFAULT 0,
    physical_write_bytes       INTEGER NOT NULL DEFAULT 0,
    logical_read_bytes         INTEGER NOT NULL DEFAULT 0,
    logical_write_bytes        INTEGER NOT NULL DEFAULT 0,
    cancelled_write_bytes      INTEGER NOT NULL DEFAULT 0,
    metadata_json              TEXT NOT NULL DEFAULT '{}'
);
CREATE TABLE patterns (
    pattern_id     TEXT PRIMARY KEY,
    signature      BLOB NOT NULL,
    exemplar_ids   TEXT NOT NULL,
    exemplar_count INTEGER NOT NULL DEFAULT 0,
    session_ids    TEXT NOT NULL,
    session_count  INTEGER NOT NULL DEFAULT 0,
    topic_tags     TEXT,
    model_id       TEXT,
    status         TEXT NOT NULL DEFAULT 'candidate',
    first_seen_at  INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL,
    project_id     TEXT
);
CREATE TABLE pending_message_completions (
    message_id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    claimed_at INTEGER,
    completed_at INTEGER NOT NULL,
    processing_ms INTEGER
, result_drawer_id TEXT, op_state TEXT NOT NULL DEFAULT 'queued' CHECK(op_state IN ('queued', 'running', 'completed', 'rejected', 'failed')), rejected_reason TEXT, failure_detail TEXT, result_json TEXT, source_hash TEXT);
CREATE TABLE pending_messages (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    source_hash TEXT NOT NULL,
    claim_token TEXT,
    claimed_at INTEGER,
    heartbeat_at INTEGER,
    retry_count INTEGER NOT NULL DEFAULT 0,
    retry_backoff_ms INTEGER NOT NULL DEFAULT 0,
    next_attempt_at INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL CHECK(status IN ('pending', 'claimed', 'done', 'failed')),
    payload TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_error TEXT
, result_drawer_id TEXT, op_state TEXT NOT NULL DEFAULT 'queued' CHECK(op_state IN ('queued', 'running', 'completed', 'rejected', 'failed')), rejected_reason TEXT, failure_detail TEXT, result_json TEXT, failure_class TEXT);
CREATE TABLE reindex_progress (
    source_path TEXT PRIMARY KEY,
    last_processed_chunk_id INTEGER,
    embedder_name TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('running', 'paused', 'done', 'failed'))
);
CREATE TABLE runtime_adoption_events (
    id TEXT PRIMARY KEY,
    track TEXT NOT NULL CHECK(track IN ('runtime_adoption', 'card_context', 'card_embedding', 'evaluator', 'research_adapter')),
    signal TEXT NOT NULL CHECK(signal IN ('used', 'accepted', 'rejected', 'miss', 'rollback', 'contradiction', 'neutral')),
    feature TEXT NOT NULL,
    query TEXT,
    context_hash TEXT,
    card_id TEXT,
    evaluator_id TEXT,
    research_report_id TEXT,
    note TEXT,
    metadata TEXT,
    created_at TEXT NOT NULL,
    FOREIGN KEY(card_id) REFERENCES knowledge_cards(id) ON DELETE SET NULL
);
CREATE TABLE runtime_writer_lease_generations (
    name TEXT PRIMARY KEY,
    last_generation INTEGER NOT NULL CHECK(last_generation > 0)
);
CREATE TABLE runtime_writer_leases (
    name TEXT NOT NULL PRIMARY KEY,
    owner TEXT NOT NULL,
    pid INTEGER NOT NULL,
    boot_id TEXT,
    session_id TEXT NOT NULL,
    acquired_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    expires_at TEXT NOT NULL,
    heartbeat_at TEXT NOT NULL,
    mode TEXT NOT NULL,
    metadata_json TEXT
, generation INTEGER NOT NULL DEFAULT 1);
CREATE TABLE skills (
    skill_id             TEXT PRIMARY KEY,
    name                 TEXT NOT NULL,
    trigger_description  TEXT NOT NULL,
    pattern_id           TEXT NOT NULL,
    exemplar_ids         TEXT NOT NULL,
    adoption_count       INTEGER NOT NULL DEFAULT 0,
    rejection_count      INTEGER NOT NULL DEFAULT 0,
    status               TEXT NOT NULL DEFAULT 'probationary',
    promoted_at          INTEGER NOT NULL,
    updated_at           INTEGER NOT NULL,
    project_id           TEXT
);
CREATE TABLE sleep_log (
    id TEXT PRIMARY KEY,
    created_at TEXT NOT NULL,
    phase TEXT NOT NULL CHECK(phase IN ('full', 'nrem', 'rem', 'salience', 'selected')),
    processed_count INTEGER NOT NULL DEFAULT 0,
    pruned_count INTEGER NOT NULL DEFAULT 0,
    compacted_count INTEGER NOT NULL DEFAULT 0,
    conflicts_resolved_count INTEGER NOT NULL DEFAULT 0,
    salience_scored_count INTEGER NOT NULL DEFAULT 0,
    dry_run INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE sleep_resolution_log (
    id TEXT PRIMARY KEY,
    created_at TEXT NOT NULL,
    wing TEXT NOT NULL,
    drawer_id TEXT NOT NULL REFERENCES drawers(id),
    contradicted_triple_id TEXT NOT NULL REFERENCES triples(id),
    contradicted_source_drawer TEXT REFERENCES drawers(id),
    new_confidence REAL NOT NULL,
    existing_confidence REAL NOT NULL,
    action TEXT NOT NULL CHECK(action IN ('invalidated')),
    dry_run INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE taxonomy (
    wing TEXT NOT NULL,
    room TEXT NOT NULL DEFAULT '',
    display_name TEXT,
    keywords TEXT,
    PRIMARY KEY (wing, room)
);
CREATE TABLE triples (
    id TEXT PRIMARY KEY,
    subject TEXT NOT NULL,
    predicate TEXT NOT NULL,
    object TEXT NOT NULL,
    valid_from TEXT,
    valid_to TEXT,
    confidence REAL DEFAULT 1.0,
    source_drawer TEXT REFERENCES drawers(id)
, project_id TEXT);
CREATE TABLE tunnels (
    id TEXT PRIMARY KEY,
    left_wing TEXT NOT NULL,
    left_room TEXT,
    right_wing TEXT NOT NULL,
    right_room TEXT,
    label TEXT NOT NULL,
    created_at TEXT NOT NULL,
    created_by TEXT,
    deleted_at TEXT
);
INSERT INTO fork_ext_meta(key,value) VALUES('fork_ext_version','28');
