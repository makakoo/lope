CREATE TABLE findings (
        id              INTEGER PRIMARY KEY AUTOINCREMENT,
        hash            TEXT UNIQUE NOT NULL,
        message         TEXT NOT NULL,
        file            TEXT,
        line            INTEGER,
        end_line        INTEGER,
        severity        TEXT NOT NULL DEFAULT 'info',
        category        TEXT,
        confidence      REAL NOT NULL DEFAULT 0.0,
        consensus_score REAL NOT NULL DEFAULT 0.0,
        consensus_level TEXT NOT NULL DEFAULT 'unverified',
        agreement_count INTEGER NOT NULL DEFAULT 0,
        total_validators INTEGER NOT NULL DEFAULT 0,
        detected_by_json TEXT NOT NULL DEFAULT '[]',
        evidence_json    TEXT NOT NULL DEFAULT '{}',
        first_seen_at    TEXT NOT NULL,
        last_seen_at     TEXT NOT NULL,
        seen_count       INTEGER NOT NULL DEFAULT 1
    );
CREATE TABLE sqlite_sequence(name,seq);
CREATE TABLE review_sessions (
        id              INTEGER PRIMARY KEY AUTOINCREMENT,
        task            TEXT NOT NULL DEFAULT '',
        focus           TEXT NOT NULL DEFAULT '',
        target_path     TEXT NOT NULL DEFAULT '',
        validators_json TEXT NOT NULL DEFAULT '[]',
        raw_count       INTEGER NOT NULL DEFAULT 0,
        merged_count    INTEGER NOT NULL DEFAULT 0,
        created_at      TEXT NOT NULL,
        duration_ms     INTEGER NOT NULL DEFAULT 0
    );
CREATE TABLE session_findings (
        session_id    INTEGER NOT NULL,
        finding_hash  TEXT NOT NULL,
        PRIMARY KEY (session_id, finding_hash)
    );
CREATE INDEX idx_findings_hash         ON findings(hash);
CREATE INDEX idx_findings_file         ON findings(file);
CREATE INDEX idx_findings_score        ON findings(consensus_score DESC);
CREATE INDEX idx_findings_last_seen    ON findings(last_seen_at DESC);
CREATE INDEX idx_session_findings_hash ON session_findings(finding_hash);
CREATE TABLE gate_sessions (
        id              INTEGER PRIMARY KEY AUTOINCREMENT,
        task            TEXT NOT NULL DEFAULT '',
        mode            TEXT NOT NULL DEFAULT '',
        baseline_path   TEXT NOT NULL DEFAULT '',
        created_at      TEXT NOT NULL,
        passed          INTEGER NOT NULL DEFAULT 0,
        gate_count      INTEGER NOT NULL DEFAULT 0,
        failed_count    INTEGER NOT NULL DEFAULT 0,
        payload_json    TEXT NOT NULL DEFAULT '{}'
    );
CREATE INDEX idx_gate_sessions_created ON gate_sessions(created_at DESC);
CREATE TABLE verdict_events (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                schema_version          TEXT    NOT NULL DEFAULT '1.0.0',
                run_id                  TEXT    NOT NULL,
                gate_id                 TEXT,
                validator_name          TEXT    NOT NULL,
                validator_version       TEXT,
                prompt_template_version TEXT    NOT NULL,
                rendered_prompt         TEXT    NOT NULL,
                task_spec_hash          TEXT    NOT NULL,
                validation_input_ref    TEXT,
                raw_response            TEXT    NOT NULL,
                parser_version          TEXT    NOT NULL,
                initial_parse_status    TEXT    NOT NULL,
                parse_error_category    TEXT,
                repair_attempted        INTEGER NOT NULL DEFAULT 0,
                repair_status           TEXT,
                repaired_response       TEXT,
                final_verdict           TEXT    NOT NULL,
                candidate_output_ref    TEXT,
                latency_s               REAL,
                exit_status             INTEGER,
                timed_out               INTEGER NOT NULL DEFAULT 0,
                created_at              TEXT    NOT NULL
            );
CREATE INDEX idx_verdict_events_run ON verdict_events(run_id);
CREATE INDEX idx_verdict_events_validator ON verdict_events(validator_name, created_at DESC);
CREATE INDEX idx_verdict_events_category ON verdict_events(parse_error_category);
CREATE INDEX idx_verdict_events_created ON verdict_events(created_at DESC);
