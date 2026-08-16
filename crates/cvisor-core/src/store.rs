//! SQLite-backed persistence (via sqlx) for daemon state, so sandboxes and
//! snapshot metadata survive a restart. Enabled by the `sqlx` feature; the
//! daemon always turns it on. Kept out of the pure-Rust FFI/CLI builds because
//! sqlx's bundled C SQLite does not cross-compile under cargo-zigbuild.

use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use sqlx::Row;

use crate::cgroup::Limits;

pub type StoreResult<T> = Result<T, sqlx::Error>;

/// A persisted sandbox: its identity, overlay uid, and configuration.
pub struct PersistedSandbox {
    pub id: String,
    pub name: String,
    pub uid: [u8; 16],
    pub allow_network: bool,
    pub allow_listen: bool,
    pub env: Vec<(String, String)>,
    pub limits: Limits,
}

/// A persisted snapshot's metadata (the bytes live on disk under
/// `/tmp/.cvisor/snapshots/{id}`).
pub struct PersistedSnapshot {
    pub id: String,
    pub source: Option<String>,
    pub size: u64,
    pub created_at: i64,
}

/// A SQLite connection pool with the cVisor schema applied.
pub struct Store {
    pool: SqlitePool,
}

impl Store {
    /// Open (creating if absent) the database at `path` and apply the schema.
    pub async fn open(path: &str) -> StoreResult<Store> {
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(opts).await?;
        let store = Store { pool };
        store.migrate().await?;
        Ok(store)
    }

    async fn migrate(&self) -> StoreResult<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS sandboxes (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                uid BLOB NOT NULL,
                allow_network INTEGER NOT NULL,
                allow_listen INTEGER NOT NULL,
                env TEXT NOT NULL,
                memory_max INTEGER,
                pids_max INTEGER,
                cpu_percent INTEGER,
                created_at INTEGER NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS snapshots (
                id TEXT PRIMARY KEY,
                source TEXT,
                size INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;
        // FTS5 full-text index over sandbox id/name/env for search.
        sqlx::query(
            "CREATE VIRTUAL TABLE IF NOT EXISTS sandbox_fts
             USING fts5(id UNINDEXED, name, env, tokenize = 'unicode61')",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Insert or update a sandbox row.
    pub async fn upsert_sandbox(&self, s: &PersistedSandbox) -> StoreResult<()> {
        let env = serde_json::to_string(&s.env).unwrap_or_else(|_| "[]".to_string());
        sqlx::query(
            "INSERT INTO sandboxes
                (id, name, uid, allow_network, allow_listen, env, memory_max, pids_max, cpu_percent, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET
                name = ?2, uid = ?3, allow_network = ?4, allow_listen = ?5,
                env = ?6, memory_max = ?7, pids_max = ?8, cpu_percent = ?9",
        )
        .bind(&s.id)
        .bind(&s.name)
        .bind(&s.uid[..])
        .bind(s.allow_network as i64)
        .bind(s.allow_listen as i64)
        .bind(env)
        .bind(s.limits.memory_max.map(|v| v as i64))
        .bind(s.limits.pids_max.map(|v| v as i64))
        .bind(s.limits.cpu_percent.map(|v| v as i64))
        .bind(now())
        .execute(&self.pool)
        .await?;
        // Keep the FTS index in sync (FTS5 has no upsert).
        let env = serde_json::to_string(&s.env).unwrap_or_default();
        sqlx::query("DELETE FROM sandbox_fts WHERE id = ?1")
            .bind(&s.id)
            .execute(&self.pool)
            .await?;
        sqlx::query("INSERT INTO sandbox_fts (id, name, env) VALUES (?1, ?2, ?3)")
            .bind(&s.id)
            .bind(&s.name)
            .bind(env)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_sandbox(&self, id: &str) -> StoreResult<()> {
        sqlx::query("DELETE FROM sandboxes WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM sandbox_fts WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Total number of persisted sandboxes.
    pub async fn count_sandboxes(&self) -> StoreResult<i64> {
        let row = sqlx::query("SELECT COUNT(*) AS n FROM sandboxes")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get("n"))
    }

    /// A page of persisted sandboxes ordered by creation. `limit < 0` = all.
    pub async fn list_sandboxes(
        &self,
        limit: i64,
        offset: i64,
    ) -> StoreResult<Vec<PersistedSandbox>> {
        let rows = sqlx::query(
            "SELECT id, name, uid, allow_network, allow_listen, env, memory_max, pids_max, cpu_percent
             FROM sandboxes ORDER BY created_at LIMIT ?1 OFFSET ?2",
        )
        .bind(limit)
        .bind(offset.max(0))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().filter_map(row_to_sandbox).collect())
    }

    /// Full-text search sandboxes (FTS5 MATCH over name/env), paginated.
    pub async fn search_sandboxes(
        &self,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> StoreResult<Vec<PersistedSandbox>> {
        if query.trim().is_empty() {
            return self.list_sandboxes(limit, offset).await;
        }
        let rows = sqlx::query(
            "SELECT s.id, s.name, s.uid, s.allow_network, s.allow_listen, s.env,
                    s.memory_max, s.pids_max, s.cpu_percent
             FROM sandboxes s
             JOIN sandbox_fts f ON f.id = s.id
             WHERE sandbox_fts MATCH ?1
             ORDER BY rank LIMIT ?2 OFFSET ?3",
        )
        .bind(fts_query(query))
        .bind(limit)
        .bind(offset.max(0))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().filter_map(row_to_sandbox).collect())
    }

    /// Record (or update) a snapshot's metadata.
    pub async fn record_snapshot(
        &self,
        id: &str,
        source: Option<&str>,
        size: u64,
    ) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO snapshots (id, source, size, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET source = ?2, size = ?3",
        )
        .bind(id)
        .bind(source)
        .bind(size as i64)
        .bind(now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_snapshot(&self, id: &str) -> StoreResult<()> {
        sqlx::query("DELETE FROM snapshots WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn count_snapshots(&self) -> StoreResult<i64> {
        let row = sqlx::query("SELECT COUNT(*) AS n FROM snapshots")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get("n"))
    }

    /// A page of snapshot metadata (newest first). `limit < 0` = all.
    pub async fn list_snapshots(
        &self,
        limit: i64,
        offset: i64,
    ) -> StoreResult<Vec<PersistedSnapshot>> {
        let rows = sqlx::query(
            "SELECT id, source, size, created_at FROM snapshots
             ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
        )
        .bind(limit)
        .bind(offset.max(0))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| PersistedSnapshot {
                id: r.get("id"),
                source: r.get("source"),
                size: r.get::<i64, _>("size") as u64,
                created_at: r.get("created_at"),
            })
            .collect())
    }
}

/// Build an FTS5 prefix AND-query from user input, quoting each token so
/// arbitrary characters can't break the MATCH syntax.
fn fts_query(q: &str) -> String {
    q.split_whitespace()
        .map(|t| format!("\"{}\"*", t.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

fn row_to_sandbox(r: sqlx::sqlite::SqliteRow) -> Option<PersistedSandbox> {
    let uid_blob: Vec<u8> = r.get("uid");
    if uid_blob.len() != 16 {
        return None;
    }
    let mut uid = [0u8; 16];
    uid.copy_from_slice(&uid_blob);
    let env_json: String = r.get("env");
    let env = serde_json::from_str(&env_json).unwrap_or_default();
    let memory_max: Option<i64> = r.get("memory_max");
    let pids_max: Option<i64> = r.get("pids_max");
    let cpu_percent: Option<i64> = r.get("cpu_percent");
    Some(PersistedSandbox {
        id: r.get("id"),
        name: r.get("name"),
        uid,
        allow_network: r.get::<i64, _>("allow_network") != 0,
        allow_listen: r.get::<i64, _>("allow_listen") != 0,
        env,
        limits: Limits {
            memory_max: memory_max.map(|v| v as u64),
            pids_max: pids_max.map(|v| v as u64),
            cpu_percent: cpu_percent.map(|v| v as u32),
        },
    })
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sandbox_roundtrip_and_delete() {
        let path = std::env::temp_dir().join(format!("cvstore-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let store = Store::open(path.to_str().unwrap()).await.unwrap();

        let sb = PersistedSandbox {
            id: "abc123".into(),
            name: "nervous_einstein".into(),
            uid: *b"0123456789abcdef",
            allow_network: false,
            allow_listen: true,
            env: vec![("FOO".into(), "bar".into())],
            limits: Limits {
                memory_max: Some(256 * 1024 * 1024),
                pids_max: None,
                cpu_percent: Some(50),
            },
        };
        store.upsert_sandbox(&sb).await.unwrap();

        let loaded = store.list_sandboxes(-1, 0).await.unwrap();
        assert_eq!(loaded.len(), 1);
        let g = &loaded[0];
        assert_eq!(g.id, "abc123");
        assert_eq!(g.name, "nervous_einstein");
        assert_eq!(g.uid, *b"0123456789abcdef");
        assert!(!g.allow_network && g.allow_listen);
        assert_eq!(g.env, vec![("FOO".to_string(), "bar".to_string())]);
        assert_eq!(g.limits.memory_max, Some(256 * 1024 * 1024));
        assert_eq!(g.limits.cpu_percent, Some(50));
        assert_eq!(store.count_sandboxes().await.unwrap(), 1);

        // FTS5 search by a name prefix and by an env value.
        assert_eq!(
            store
                .search_sandboxes("nervous", -1, 0)
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            store.search_sandboxes("einst", -1, 0).await.unwrap().len(),
            1
        );
        assert!(store
            .search_sandboxes("nomatch", -1, 0)
            .await
            .unwrap()
            .is_empty());

        // Pagination: a second sandbox, then a page of 1.
        let mut sb2 = sb;
        sb2.id = "def456".into();
        sb2.name = "happy_curie".into();
        store.upsert_sandbox(&sb2).await.unwrap();
        assert_eq!(store.count_sandboxes().await.unwrap(), 2);
        assert_eq!(store.list_sandboxes(1, 0).await.unwrap().len(), 1);
        assert_eq!(store.list_sandboxes(1, 1).await.unwrap().len(), 1);
        assert_eq!(store.list_sandboxes(1, 2).await.unwrap().len(), 0);

        store.delete_sandbox("abc123").await.unwrap();
        store.delete_sandbox("def456").await.unwrap();
        assert!(store.list_sandboxes(-1, 0).await.unwrap().is_empty());
        let _ = std::fs::remove_file(&path);
    }
}
