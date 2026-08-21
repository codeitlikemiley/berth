use std::io::Write;
use std::path::{Path, PathBuf};
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
    container_id TEXT,
    created_at INTEGER NOT NULL,
    stopped_at INTEGER
);
"#;

const MAX_BEARERS: i64 = 8;

const LEASE_SELECT: &str = "SELECT leases.id, leases.session_id, leases.status, leases.quote_json,
        leases.ws_url, leases.viewer_url, leases.started_at, leases.min_seconds,
        leases.elapsed_seconds, leases.billable_seconds, sessions.container_id,
        leases.stopped_at, leases.workspace_id
 FROM leases
 LEFT JOIN sessions ON sessions.id = leases.session_id";

pub(crate) fn billable_seconds(min_seconds: u64, elapsed: u64) -> u64 {
    elapsed.max(min_seconds)
}

pub(crate) struct Db {
    conn: Mutex<Connection>,
    pair_file: PathBuf,
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
    pub container_id: Option<String>,
    pub stopped_at: Option<i64>,
    pub workspace_id: String,
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
    pub container_id: Option<String>,
}

impl Db {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            secure_mkdir(parent)?;
        }
        let pair_file = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .join("pair.code");
        let conn = Connection::open(path)?;
        secure_file(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        let _ = conn.execute("ALTER TABLE sessions ADD COLUMN container_id TEXT", []);
        secure_file(&sidecar(path, "-wal"))?;
        secure_file(&sidecar(path, "-shm"))?;
        Ok(Self {
            conn: Mutex::new(conn),
            pair_file,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("db lock poisoned")
    }

    pub(crate) fn ensure_pairing_code(&self) -> Result<String> {
        if let Ok(raw) = std::fs::read_to_string(&self.pair_file) {
            let code = raw.trim();
            if !code.is_empty() {
                self.store_pairing_hash(code)?;
                return Ok(code.to_string());
            }
        }
        if let Some(legacy) = self.take_legacy_pairing_secret()? {
            write_secret_file(&self.pair_file, &legacy)?;
            self.store_pairing_hash(&legacy)?;
            return Ok(legacy);
        }
        let code = random_pairing_code()?;
        write_secret_file(&self.pair_file, &code)?;
        self.store_pairing_hash(&code)?;
        Ok(code)
    }

    fn take_legacy_pairing_secret(&self) -> Result<Option<String>> {
        let conn = self.lock();
        let secret: Option<String> = conn
            .query_row(
                "SELECT secret FROM pair_tokens
                 WHERE kind = 'pairing_code' AND secret != '' AND revoked_at IS NULL
                 LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if secret.is_some() {
            conn.execute(
                "UPDATE pair_tokens SET secret = '' WHERE kind = 'pairing_code'",
                [],
            )?;
        }
        Ok(secret
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()))
    }

    fn store_pairing_hash(&self, code: &str) -> Result<()> {
        let hash = sha256_hex(&normalize_code(code));
        let now = unix_now();
        let mut conn = self.lock();
        let existing: Option<String> = conn
            .query_row(
                "SELECT secret_hash FROM pair_tokens
                 WHERE kind = 'pairing_code' AND revoked_at IS NULL
                 LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if existing.as_deref() == Some(hash.as_str()) {
            conn.execute(
                "UPDATE pair_tokens SET secret = ''
                 WHERE kind = 'pairing_code' AND secret != ''",
                [],
            )?;
            return Ok(());
        }
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE pair_tokens SET revoked_at = ?1
             WHERE kind = 'pairing_code' AND revoked_at IS NULL",
            params![now],
        )?;
        tx.execute(
            "INSERT INTO pair_tokens (kind, secret, secret_hash, created_at)
             VALUES ('pairing_code', '', ?1, ?2)",
            params![hash, now],
        )?;
        tx.commit()?;
        Ok(())
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

    pub(crate) fn issue_bearer(&self, revoke_others: bool) -> Result<String> {
        let token = random_bearer()?;
        let hash = sha256_hex(&token);
        let now = unix_now();
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        if revoke_others {
            tx.execute(
                "UPDATE pair_tokens SET revoked_at = ?1
                 WHERE kind = 'bearer' AND revoked_at IS NULL",
                params![now],
            )?;
        } else {
            let n: i64 = tx.query_row(
                "SELECT COUNT(*) FROM pair_tokens
                 WHERE kind = 'bearer' AND revoked_at IS NULL",
                [],
                |row| row.get(0),
            )?;
            if n >= MAX_BEARERS {
                return Err(Error::TooManyBearers);
            }
        }
        tx.execute(
            "INSERT INTO pair_tokens (kind, secret, secret_hash, created_at)
             VALUES ('bearer', '', ?1, ?2)",
            params![hash, now],
        )?;
        tx.commit()?;
        Ok(token)
    }

    pub(crate) fn active_bearers(&self) -> Result<u64> {
        let n: i64 = self.lock().query_row(
            "SELECT COUNT(*) FROM pair_tokens
             WHERE kind = 'bearer' AND revoked_at IS NULL",
            [],
            |row| row.get(0),
        )?;
        Ok(u64_from_i64(n))
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
                id, lease_id, workspace_id, status, viewer_port, container_id, created_at
             ) VALUES (?1, ?2, ?3, 'active', ?4, ?5, ?6)",
            params![
                lease.session_id,
                lease.lease_id,
                lease.workspace_id,
                viewer_port,
                lease.container_id,
                now,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn get_lease(&self, lease_id: &str) -> Result<LeaseRow> {
        get_lease_conn(&self.lock(), lease_id)
    }

    pub(crate) fn list_leases(&self) -> Result<Vec<LeaseRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!("{LEASE_SELECT} ORDER BY leases.started_at DESC"))?;
        let rows = stmt.query_map([], lease_from_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub(crate) fn active_leases(&self) -> Result<Vec<LeaseRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!("{LEASE_SELECT} WHERE leases.status = 'active'"))?;
        let rows = stmt.query_map([], lease_from_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
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
        &format!("{LEASE_SELECT} WHERE leases.id = ?1"),
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
        container_id: row.get(10)?,
        stopped_at: row.get(11)?,
        workspace_id: row.get(12)?,
    })
}

fn sidecar(db: &Path, suffix: &str) -> PathBuf {
    let mut s = db.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

fn secure_mkdir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    set_mode(path, 0o700)
}

fn secure_file(path: &Path) -> Result<()> {
    if path.exists() {
        set_mode(path, 0o600)?;
    }
    Ok(())
}

fn write_secret_file(path: &Path, contents: &str) -> Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(path)?;
    file.write_all(contents.as_bytes())?;
    file.write_all(b"\n")?;
    file.flush()?;
    secure_file(path)?;
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(mode);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
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
    fn pairing_code_stable_and_multi_bearer() {
        let (dir, db) = tmp_db();
        let a = db.ensure_pairing_code().unwrap();
        let b = db.ensure_pairing_code().unwrap();
        assert_eq!(a, b);
        let stored = std::fs::read_to_string(dir.path().join("pair.code")).unwrap();
        assert_eq!(stored.trim(), a);
        let conn = Connection::open(dir.path().join("node.db")).unwrap();
        let secret: String = conn
            .query_row(
                "SELECT secret FROM pair_tokens
                 WHERE kind = 'pairing_code' AND revoked_at IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(secret, "");
        assert!(db.check_pair_code(&a).unwrap());
        assert!(db.check_pair_code(&a.to_lowercase()).unwrap());
        assert!(!db.check_pair_code("nope-nope").unwrap());

        let t1 = db.issue_bearer(false).unwrap();
        assert!(db.bearer_valid(&t1).unwrap());
        let t2 = db.issue_bearer(false).unwrap();
        assert_ne!(t1, t2);
        assert!(db.bearer_valid(&t1).unwrap());
        assert!(db.bearer_valid(&t2).unwrap());
        assert_eq!(db.active_bearers().unwrap(), 2);
        assert!(!db.bearer_valid(&a).unwrap());

        let t3 = db.issue_bearer(true).unwrap();
        assert!(!db.bearer_valid(&t1).unwrap());
        assert!(!db.bearer_valid(&t2).unwrap());
        assert!(db.bearer_valid(&t3).unwrap());
        assert_eq!(db.active_bearers().unwrap(), 1);
    }

    #[test]
    fn ninth_bearer_without_revoke_is_too_many() {
        let (_dir, db) = tmp_db();
        let mut tokens = Vec::new();
        for _ in 0..MAX_BEARERS {
            tokens.push(db.issue_bearer(false).unwrap());
        }
        for t in &tokens {
            assert!(db.bearer_valid(t).unwrap());
        }
        assert!(matches!(db.issue_bearer(false), Err(Error::TooManyBearers)));
        let rotated = db.issue_bearer(true).unwrap();
        for t in &tokens {
            assert!(!db.bearer_valid(t).unwrap());
        }
        assert!(db.bearer_valid(&rotated).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn data_dir_and_secrets_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let nest = dir.path().join("berth");
        let db = Db::open(&nest.join("node.db")).unwrap();
        db.ensure_pairing_code().unwrap();
        let dir_mode = std::fs::metadata(&nest).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
        let db_mode = std::fs::metadata(nest.join("node.db"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(db_mode, 0o600);
        let code_mode = std::fs::metadata(nest.join("pair.code"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(code_mode, 0o600);
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
            container_id: Some("abc123".into()),
        })
        .unwrap();
        let row = db.get_lease("l_1").unwrap();
        assert_eq!(row.status, "active");
        assert_eq!(row.container_id.as_deref(), Some("abc123"));
        assert_eq!(row.workspace_id, "ws_1");
        assert!(row.billable_seconds.is_none());
        assert!(row.stopped_at.is_none());
        let stopped = db.stop_lease("l_1").unwrap();
        assert_eq!(stopped.status, "stopped");
        assert_eq!(stopped.billable_seconds, Some(60));
        assert!(stopped.stopped_at.is_some());
        let listed = db.list_leases().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "l_1");
        assert_eq!(listed[0].status, "stopped");
        let again = db.stop_lease("l_1").unwrap();
        assert_eq!(again.billable_seconds, Some(60));
        assert!(matches!(db.get_lease("missing"), Err(Error::NotFound)));
    }
}
