//! Real multi-process WAL concurrency stress test for the storage layer.
//!
//! The test re-executes its own test binary as 12 independent OS processes
//! (via the `AIKD_WAL_WORKER` self-re-exec pattern). Every process opens the
//! same temp SQLite database in WAL mode, runs 20 mixed operations
//! (`events::append_event` + `memory::save_memory` + read/query), and exits.
//!
//! The coordinator then asserts:
//! - all 12 child processes exited successfully (no panics, no `SQLITE_BUSY`),
//! - every expected event and memory row was persisted,
//! - event sequences are dense, unique and monotonic (`1..=240`),
//! - SQLite `integrity_check` reports `ok`.

use std::path::Path;
use std::process::{Command, Stdio};

use aikd_storage::{events, memory, Database};
use rusqlite::params;

const WORKER_ENV: &str = "AIKD_WAL_WORKER";
const DB_ENV: &str = "AIKD_WAL_DB";
const WORKERS: usize = 12;
const OPS_PER_WORKER: usize = 20;
const PROJECT: &str = "/wal/stress";
const WORKSTREAM: &str = "wal-stress";

#[test]
fn wal_multiprocess_concurrent_writes_and_reads() {
    if let (Ok(worker), Ok(db_path)) = (std::env::var(WORKER_ENV), std::env::var(DB_ENV)) {
        let worker: usize = worker.parse().expect("AIKD_WAL_WORKER must be a number");
        run_worker(worker, Path::new(&db_path));
        return;
    }
    run_coordinator();
}

/// Child-process body: 20 mixed operations against the shared WAL database.
fn run_worker(worker: usize, db_path: &Path) {
    let db = Database::open(db_path).expect("worker must open the shared database");
    let conn = db.conn();

    // The database is intentionally contended: give SQLite a generous busy
    // window so the assertion below fails only on a genuine locking defect.
    conn.execute_batch("PRAGMA busy_timeout=30000")
        .expect("worker must configure busy_timeout");

    let journal_mode: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .expect("worker must read journal_mode");
    assert_eq!(
        journal_mode.to_ascii_lowercase(),
        "wal",
        "worker {worker} must use WAL journaling"
    );

    for op in 0..OPS_PER_WORKER {
        let source_ref = format!("w{worker:02}-op{op:02}");
        events::append_event(
            conn,
            PROJECT,
            WORKSTREAM,
            "wal-worker",
            &format!("session-{worker}"),
            "tool_call",
            &source_ref,
            &format!(r#"{{"worker":{worker},"op":{op}}}"#),
        )
        .expect("append_event must not fail under multi-process WAL contention");

        memory::save_memory(
            conn,
            PROJECT,
            WORKSTREAM,
            "fact",
            &format!("worker {worker} fact {op}"),
            &format!("payload from worker {worker}, operation {op}"),
            "agent-asserted",
            0.9,
        )
        .expect("save_memory must not fail under multi-process WAL contention");

        // Interleave reads/queries with the writes so readers race writers.
        let latest = events::latest_sequence(conn, PROJECT, WORKSTREAM)
            .expect("latest_sequence must be readable during writes");
        assert!(
            latest >= 1,
            "worker {worker} must observe its own journal writes"
        );

        let listed = events::list_events(conn, PROJECT, WORKSTREAM, None, 0, 1000)
            .expect("list_events must be readable during writes");
        assert!(
            !listed.is_empty(),
            "worker {worker} must list journal events during writes"
        );

        let memories = memory::list_memory(conn, PROJECT, Some(WORKSTREAM), None, false)
            .expect("list_memory must be readable during writes");
        assert!(
            !memories.is_empty(),
            "worker {worker} must list memories during writes"
        );
    }
}

/// Parent-process body: prepare the database, spawn 12 OS processes, verify.
fn run_coordinator() {
    let temp = tempfile::tempdir().expect("create temp dir for WAL stress database");
    let db_path = temp.path().join("wal_stress.db");

    // Migrate once up-front so children never race DDL-heavy migrations.
    {
        let db = Database::open(&db_path).expect("create and migrate stress database");
        let journal_mode: String = db
            .conn()
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .expect("read journal_mode");
        assert_eq!(
            journal_mode.to_ascii_lowercase(),
            "wal",
            "stress database must start in WAL mode"
        );
    }

    let exe = std::env::current_exe().expect("resolve current test executable");
    let mut children = Vec::with_capacity(WORKERS);
    for worker in 0..WORKERS {
        let child = Command::new(&exe)
            .arg("wal_multiprocess_concurrent_writes_and_reads")
            .arg("--exact")
            .arg("--nocapture")
            .env(WORKER_ENV, worker.to_string())
            .env(DB_ENV, &db_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|err| panic!("failed to spawn WAL worker {worker}: {err}"));
        children.push(child);
    }

    for (worker, child) in children.into_iter().enumerate() {
        let output = child
            .wait_with_output()
            .unwrap_or_else(|err| panic!("failed to wait for WAL worker {worker}: {err}"));
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "WAL worker {worker} exited with {:?}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}",
            output.status
        );
        assert_eq!(
            output.status.code(),
            Some(0),
            "WAL worker {worker} must exit with code 0"
        );
        assert!(
            !stderr.contains("SQLITE_BUSY"),
            "WAL worker {worker} reported SQLITE_BUSY\n--- stderr ---\n{stderr}"
        );
        assert!(
            !stderr.contains("panicked at"),
            "WAL worker {worker} panicked\n--- stderr ---\n{stderr}"
        );
    }

    let db = Database::open(&db_path).expect("reopen stress database for verification");
    let conn = db.conn();

    let expected_rows = (WORKERS * OPS_PER_WORKER) as i64;

    let event_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE project_id = ?1",
            params![PROJECT],
            |row| row.get(0),
        )
        .expect("count stress events");
    assert_eq!(
        event_count, expected_rows,
        "all {expected_rows} events must be persisted"
    );

    let memory_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory WHERE project_path = ?1",
            params![PROJECT],
            |row| row.get(0),
        )
        .expect("count stress memories");
    assert_eq!(
        memory_count, expected_rows,
        "all {expected_rows} memory rows must be persisted"
    );

    let mut stmt = conn
        .prepare("SELECT sequence FROM events WHERE project_id = ?1 ORDER BY sequence ASC")
        .expect("prepare sequence query");
    let sequences: Vec<i64> = stmt
        .query_map(params![PROJECT], |row| row.get(0))
        .expect("query event sequences")
        .collect::<std::result::Result<Vec<i64>, _>>()
        .expect("collect event sequences");
    assert_eq!(sequences.len(), expected_rows as usize);
    assert_eq!(
        sequences,
        (1..=expected_rows).collect::<Vec<i64>>(),
        "event sequences must be unique, dense and monotonic"
    );

    let latest = events::latest_sequence(conn, PROJECT, WORKSTREAM).expect("latest sequence");
    assert_eq!(
        latest, expected_rows,
        "latest_sequence must expose the full stress watermark"
    );

    let journal_count = events::list_events(conn, PROJECT, WORKSTREAM, None, 0, 10_000)
        .expect("list all stress events")
        .len();
    assert_eq!(journal_count, expected_rows as usize);

    for worker in 0..WORKERS {
        let event_prefix = format!("w{worker:02}-op%");
        let worker_events: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE project_id = ?1 AND source_ref LIKE ?2",
                params![PROJECT, event_prefix],
                |row| row.get(0),
            )
            .expect("count worker events");
        assert_eq!(
            worker_events, OPS_PER_WORKER as i64,
            "worker {worker} must persist exactly {OPS_PER_WORKER} events"
        );

        let memory_prefix = format!("worker {worker} fact %");
        let worker_memories: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memory WHERE project_path = ?1 AND title LIKE ?2",
                params![PROJECT, memory_prefix],
                |row| row.get(0),
            )
            .expect("count worker memories");
        assert_eq!(
            worker_memories, OPS_PER_WORKER as i64,
            "worker {worker} must persist exactly {OPS_PER_WORKER} memory rows"
        );
    }

    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .expect("run integrity_check");
    assert_eq!(integrity, "ok", "SQLite integrity_check must pass");

    println!(
        "WAL stress OK: {WORKERS} processes x {OPS_PER_WORKER} ops -> \
         {event_count} events ({memory_count} memories), sequences 1..={expected_rows}"
    );
}
