use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};

use crate::docker::volume_name;
use crate::error::{Error, Result};
use crate::id::{
    normalize_code, random_bearer, random_pairing_code, sha256_hex, u64_from_i64, unix_now,
};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS pair_tokens (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    secret TEXT NOT NULL,
    secret_hash TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    revoked_at INTEGER
);

CREATE TABLE IF NOT EXISTS workspaces (
    id TEXT PRIMARY KEY,
    volume TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS leases (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    status TEXT NOT NULL,
    request_json TEXT NOT NULL,
    quote_json TEXT NOT NULL,
    ws_url TEXT NOT NULL,
    viewer_url TEXT,
    started_at INTEGER NOT NULL,
    stopped_at INTEGER,
    min_seconds INTEGER NOT NULL,
    elapsed_seconds INTEGER,
    billable_seconds INTEGER
);

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    lease_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    status TEXT NOT NULL,
    viewer_port INTEGER,
    created_at INTEGER NOT NULL,
    stopped_at INTEGER
);
"#;

pub(crate) fn billable_seconds(min_seconds: u64, elapsed: u64) -> u64 {
    elapsed.max(min_seconds)
}

pub(crate) struct Db {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone)]
pub(crate) struct LeaseRow {
    pub id: String,
    pub session_id: String,
    pub status: String,
    pub quote_json: String,
    pub ws_url: String,
    pub viewer_url: Option<String>,
    pub started_at: i64,
    pub min_seconds: i64,
    pub elapsed_seconds: Option<i64>,
    pub billable_seconds: Option<i64>,
}

pub(crate) struct NewLease {
    pub lease_id: String,
    pub session_id: String,
    pub workspace_id: String,
    pub request_json: String,
    pub quote_json: String,
    pub ws_url: String,
    pub viewer_url: Option<String>,
    pub min_seconds: u64,
    pub viewer_port: Option<u16>,
}

impl Db {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("db lock poisoned")
    }

    pub(crate) fn ensure_pairing_code(&self) -> Result<String> {
        let conn = self.lock();
        let existing: Option<String> = conn
            .query_row(
                "SELECT secret FROM pair_tokens
                 WHERE kind = 'pairing_code' AND revoked_at IS NULL
                 LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(code) = existing {
            return Ok(code);
        }
        let code = random_pairing_code()?;
        let hash = sha256_hex(&normalize_code(&code));
        let now = unix_now();
        conn.execute(
            "INSERT INTO pair_tokens (kind, secret, secret_hash, created_at)
             VALUES ('pairing_code', ?1, ?2, ?3)",
            params![code, hash, now],
        )?;
        Ok(code)
    }

    pub(crate) fn pairing_code(&self) -> Result<String> {
        self.ensure_pairing_code()
    }

    pub(crate) fn check_pair_code(&self, code: &str) -> Result<bool> {
        let normalized = normalize_code(code);
        if normalized.is_empty() {
            return Ok(false);
        }
        let hash = sha256_hex(&normalized);
        let found: Option<i64> = self
            .lock()
            .query_row(
                "SELECT id FROM pair_tokens
                 WHERE kind = 'pairing_code' AND secret_hash = ?1 AND revoked_at IS NULL
                 LIMIT 1",
                params![hash],
                |row| row.get(0),
            )
            .optional()?;
        Ok(found.is_some())
    }

    /// Rotate: revoke existing bearer tokens, issue a new one. Returns plaintext once.
    pub(crate) fn issue_bearer(&self) -> Result<String> {
        let token = random_bearer()?;
        let hash = sha256_hex(&token);
        let now = unix_now();
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE pair_tokens SET revoked_at = ?1
             WHERE kind = 'bearer' AND revoked_at IS NULL",
            params![now],
        )?;
        tx.execute(
            "INSERT INTO pair_tokens (kind, secret, secret_hash, created_at)
             VALUES ('bearer', '', ?1, ?2)",
            params![hash, now],
        )?;
        tx.commit()?;
        Ok(token)
    }

    pub(crate) fn bearer_valid(&self, token: &str) -> Result<bool> {
        if token.is_empty() {
            return Ok(false);
        }
        let hash = sha256_hex(token);
        let found: Option<i64> = self
            .lock()
            .query_row(
                "SELECT id FROM pair_tokens
                 WHERE kind = 'bearer' AND secret_hash = ?1 AND revoked_at IS NULL
                 LIMIT 1",
                params![hash],
                |row| row.get(0),
            )
            .optional()?;
        Ok(found.is_some())
    }

    pub(crate) fn insert_lease(&self, lease: &NewLease) -> Result<()> {
        let now = unix_now();
        let volume = volume_name(&lease.workspace_id);
        let viewer_port = lease.viewer_port.map(i64::from);
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO workspaces (id, volume, created_at) VALUES (?1, ?2, ?3)",
            params![lease.workspace_id, volume, now],
        )?;
        tx.execute(
            "INSERT INTO leases (
                id, session_id, workspace_id, status, request_json, quote_json,
                ws_url, viewer_url, started_at, min_seconds
             ) VALUES (?1, ?2, ?3, 'active', ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                lease.lease_id,
                lease.session_id,
                lease.workspace_id,
                lease.request_json,
                lease.quote_json,
                lease.ws_url,
                lease.viewer_url,
                now,
                lease.min_seconds as i64,
            ],
        )?;
        tx.execute(
            "INSERT INTO sessions (
                id, lease_id, workspace_id, status, viewer_port, created_at
             ) VALUES (?1, ?2, ?3, 'active', ?4, ?5)",
            params![
                lease.session_id,
                lease.lease_id,
                lease.workspace_id,
                viewer_port,
                now,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn get_lease(&self, lease_id: &str) -> Result<LeaseRow> {
        get_lease_conn(&self.lock(), lease_id)
    }

    pub(crate) fn stop_lease(&self, lease_id: &str) -> Result<LeaseRow> {
        let now = unix_now();
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let row = get_lease_conn(&tx, lease_id)?;
        if row.status == "stopped" {
            return Ok(row);
        }
        let elapsed = u64_from_i64(now.saturating_sub(row.started_at));
        let min = u64_from_i64(row.min_seconds);
        let billable = billable_seconds(min, elapsed);
        tx.execute(
            "UPDATE leases
             SET status = 'stopped', stopped_at = ?1, elapsed_seconds = ?2, billable_seconds = ?3
             WHERE id = ?4",
            params![now, elapsed as i64, billable as i64, lease_id],
        )?;
        tx.execute(
            "UPDATE sessions SET status = 'stopped', stopped_at = ?1 WHERE id = ?2",
            params![now, row.session_id],
        )?;
        tx.commit()?;
        get_lease_conn(&conn, lease_id)
    }
}

fn get_lease_conn(conn: &Connection, lease_id: &str) -> Result<LeaseRow> {
    conn.query_row(
        "SELECT id, session_id, status, quote_json, ws_url, viewer_url,
                started_at, min_seconds, elapsed_seconds, billable_seconds
         FROM leases WHERE id = ?1",
        params![lease_id],
        lease_from_row,
    )
    .map_err(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => Error::NotFound,
        other => Error::Db(other),
    })
}

fn lease_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LeaseRow> {
    Ok(LeaseRow {
        id: row.get(0)?,
        session_id: row.get(1)?,
        status: row.get(2)?,
        quote_json: row.get(3)?,
        ws_url: row.get(4)?,
        viewer_url: row.get(5)?,
        started_at: row.get(6)?,
        min_seconds: row.get(7)?,
        elapsed_seconds: row.get(8)?,
        billable_seconds: row.get(9)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn tmp_db() -> (tempfile::TempDir, Db) {
        let dir = tempdir().unwrap();
        let db = Db::open(&dir.path().join("node.db")).unwrap();
        (dir, db)
    }

    #[test]
    fn pairing_code_stable_and_bearer_rotates() {
        let (_dir, db) = tmp_db();
        let a = db.ensure_pairing_code().unwrap();
        let b = db.ensure_pairing_code().unwrap();
        assert_eq!(a, b);
        assert!(db.check_pair_code(&a).unwrap());
        assert!(db.check_pair_code(&a.to_lowercase()).unwrap());
        assert!(!db.check_pair_code("nope-nope").unwrap());

        let t1 = db.issue_bearer().unwrap();
        assert!(db.bearer_valid(&t1).unwrap());
        let t2 = db.issue_bearer().unwrap();
        assert_ne!(t1, t2);
        assert!(!db.bearer_valid(&t1).unwrap());
        assert!(db.bearer_valid(&t2).unwrap());
        assert!(!db.bearer_valid(&a).unwrap());
    }

    #[test]
    fn billable_floor() {
        assert_eq!(billable_seconds(60, 0), 60);
        assert_eq!(billable_seconds(60, 12), 60);
        assert_eq!(billable_seconds(60, 90), 90);
    }

    #[test]
    fn lease_stop_records_seconds() {
        let (_dir, db) = tmp_db();
        db.insert_lease(&NewLease {
            lease_id: "l_1".into(),
            session_id: "s_1".into(),
            workspace_id: "ws_1".into(),
            request_json: "{}".into(),
            quote_json: "{}".into(),
            ws_url: "ws://127.0.0.1:7432/v1/sessions/s_1".into(),
            viewer_url: Some("http://127.0.0.1:6080/vnc.html".into()),
            min_seconds: 60,
            viewer_port: Some(6080),
        })
        .unwrap();
        let row = db.get_lease("l_1").unwrap();
        assert_eq!(row.status, "active");
        assert!(row.billable_seconds.is_none());
        let stopped = db.stop_lease("l_1").unwrap();
        assert_eq!(stopped.status, "stopped");
        assert_eq!(stopped.billable_seconds, Some(60));
        let again = db.stop_lease("l_1").unwrap();
        assert_eq!(again.billable_seconds, Some(60));
        assert!(matches!(db.get_lease("missing"), Err(Error::NotFound)));
    }
}
