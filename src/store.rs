use crate::detect::{LogLevel, Severity};
use crate::redact;
use crate::time;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter, types::Value};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug)]
pub struct Store {
    conn: Connection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceInfo {
    pub name: String,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub run_count: i64,
    pub last_command: Option<String>,
    pub last_cwd: Option<String>,
    pub active: bool,
    pub status: String,
    pub active_run_count: i64,
    pub last_started_at: Option<String>,
    pub last_ended_at: Option<String>,
    pub last_exit_code: Option<i64>,
    pub last_pid: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEvent {
    pub id: String,
    pub run_id: String,
    pub source: String,
    pub ts: String,
    pub stream: String,
    pub level: String,
    pub message: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBlock {
    pub id: String,
    pub run_id: String,
    pub source: String,
    pub start_ts: String,
    pub end_ts: String,
    pub severity: String,
    pub title: String,
    pub body: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub name: String,
    pub ts: String,
}

impl Store {
    pub fn open_default() -> Result<Self> {
        Self::open(Self::default_path()?)
    }

    pub fn data_dir() -> Result<PathBuf> {
        let dir = if let Ok(home) = std::env::var("RUNAWARE_HOME") {
            PathBuf::from(home)
        } else {
            dirs::home_dir()
                .context("could not resolve home directory")?
                .join(".runaware")
        };
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    pub fn default_path() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join("runaware.db"))
    }

    pub fn open(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS sources (
              name TEXT PRIMARY KEY,
              first_seen_at TEXT NOT NULL,
              last_seen_at TEXT NOT NULL,
              run_count INTEGER NOT NULL DEFAULT 0,
              last_command TEXT,
              last_cwd TEXT
            );

            CREATE TABLE IF NOT EXISTS runs (
              id TEXT PRIMARY KEY,
              source TEXT NOT NULL,
              command TEXT NOT NULL,
              cwd TEXT NOT NULL,
              started_at TEXT NOT NULL,
              ended_at TEXT,
              exit_code INTEGER,
              pid INTEGER,
              FOREIGN KEY(source) REFERENCES sources(name)
            );

            CREATE TABLE IF NOT EXISTS log_events (
              id TEXT PRIMARY KEY,
              run_id TEXT NOT NULL,
              source TEXT NOT NULL,
              ts TEXT NOT NULL,
              stream TEXT NOT NULL,
              level TEXT NOT NULL,
              message TEXT NOT NULL,
              tags TEXT NOT NULL,
              FOREIGN KEY(run_id) REFERENCES runs(id)
            );

            CREATE TABLE IF NOT EXISTS error_blocks (
              id TEXT PRIMARY KEY,
              run_id TEXT NOT NULL,
              source TEXT NOT NULL,
              start_ts TEXT NOT NULL,
              end_ts TEXT NOT NULL,
              severity TEXT NOT NULL,
              title TEXT NOT NULL,
              body TEXT NOT NULL,
              fingerprint TEXT NOT NULL,
              FOREIGN KEY(run_id) REFERENCES runs(id)
            );

            CREATE TABLE IF NOT EXISTS checkpoints (
              id TEXT PRIMARY KEY,
              name TEXT NOT NULL,
              ts TEXT NOT NULL
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS log_fts USING fts5(
              event_id UNINDEXED,
              source UNINDEXED,
              message
            );

            CREATE INDEX IF NOT EXISTS idx_logs_ts ON log_events(ts);
            CREATE INDEX IF NOT EXISTS idx_logs_source_ts ON log_events(source, ts);
            CREATE INDEX IF NOT EXISTS idx_errors_ts ON error_blocks(start_ts);
            CREATE INDEX IF NOT EXISTS idx_errors_source_ts ON error_blocks(source, start_ts);
            "#,
        )?;
        self.add_column_if_missing("sources", "last_cwd", "TEXT")?;
        self.add_column_if_missing("runs", "pid", "INTEGER")?;
        self.normalize_package_manager_sources()?;
        Ok(())
    }

    fn add_column_if_missing(&self, table: &str, column: &str, column_type: &str) -> Result<()> {
        let mut stmt = self.conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
        for existing in columns {
            if existing? == column {
                return Ok(());
            }
        }

        self.conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {column_type}"),
            [],
        )?;
        Ok(())
    }

    fn normalize_package_manager_sources(&self) -> Result<()> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source, command, cwd FROM runs WHERE source IN ('npm', 'pnpm', 'yarn', 'bun')",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;

        let mut updates = Vec::new();
        for row in rows {
            let (run_id, source, command, cwd) = row?;
            if !is_package_manager_command(&source, &command) {
                continue;
            }
            let Some(new_source) = source_from_cwd(&cwd) else {
                continue;
            };
            if new_source != source {
                updates.push((run_id, new_source));
            }
        }
        drop(stmt);

        for (run_id, new_source) in updates {
            self.ensure_source(&new_source)?;
            self.conn.execute(
                "UPDATE runs SET source = ?1 WHERE id = ?2",
                params![new_source, run_id],
            )?;
            self.conn.execute(
                "UPDATE log_events SET source = ?1 WHERE run_id = ?2",
                params![new_source, run_id],
            )?;
            self.conn.execute(
                "UPDATE error_blocks SET source = ?1 WHERE run_id = ?2",
                params![new_source, run_id],
            )?;
        }

        self.rebuild_sources_from_runs()?;
        Ok(())
    }

    fn ensure_source(&self, source: &str) -> Result<()> {
        let now = time::now().to_rfc3339();
        self.conn.execute(
            r#"
            INSERT INTO sources(name, first_seen_at, last_seen_at, run_count)
            VALUES (?1, ?2, ?2, 0)
            ON CONFLICT(name) DO NOTHING
            "#,
            params![source, now],
        )?;
        Ok(())
    }

    fn rebuild_sources_from_runs(&self) -> Result<()> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT r.source, MIN(r.started_at), MAX(r.started_at), COUNT(*)
            FROM runs r
            GROUP BY r.source
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;

        let mut summaries = Vec::new();
        for row in rows {
            let (source, first_seen_at, last_seen_at, run_count) = row?;
            let (last_command, last_cwd) = self
                .latest_run_metadata_for_source(&source)?
                .unwrap_or_default();
            summaries.push((
                source,
                first_seen_at,
                last_seen_at,
                run_count,
                last_command,
                last_cwd,
            ));
        }
        drop(stmt);

        self.conn.execute(
            "DELETE FROM sources WHERE name NOT IN (SELECT DISTINCT source FROM runs)",
            [],
        )?;
        for (source, first_seen_at, last_seen_at, run_count, last_command, last_cwd) in summaries {
            self.conn.execute(
                r#"
                INSERT INTO sources(name, first_seen_at, last_seen_at, run_count, last_command, last_cwd)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ON CONFLICT(name) DO UPDATE SET
                  first_seen_at = excluded.first_seen_at,
                  last_seen_at = excluded.last_seen_at,
                  run_count = excluded.run_count,
                  last_command = excluded.last_command,
                  last_cwd = excluded.last_cwd
                "#,
                params![source, first_seen_at, last_seen_at, run_count, last_command, last_cwd],
            )?;
        }

        Ok(())
    }

    fn latest_run_metadata_for_source(&self, source: &str) -> Result<Option<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT command, cwd FROM runs WHERE source = ?1 ORDER BY started_at DESC LIMIT 1",
        )?;
        Ok(stmt
            .query_row(params![source], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()?)
    }

    pub fn start_run(&self, source: &str, command: &str, cwd: &str) -> Result<String> {
        let now = time::now().to_rfc3339();
        let command = redact::redact(command);
        self.clear_source_runtime_data(source)?;
        self.conn.execute(
            r#"
            INSERT INTO sources(name, first_seen_at, last_seen_at, run_count, last_command, last_cwd)
            VALUES (?1, ?2, ?2, 1, ?3, ?4)
            ON CONFLICT(name) DO UPDATE SET
              last_seen_at = excluded.last_seen_at,
              run_count = run_count + 1,
              last_command = excluded.last_command,
              last_cwd = excluded.last_cwd
            "#,
            params![source, now, command, cwd],
        )?;

        let id = Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO runs(id, source, command, cwd, started_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, source, command, cwd, now],
        )?;
        Ok(id)
    }

    pub fn finish_run(&self, run_id: &str, exit_code: i32) -> Result<()> {
        let now = time::now().to_rfc3339();
        self.conn.execute(
            "UPDATE runs SET ended_at = ?1, exit_code = ?2 WHERE id = ?3",
            params![now, exit_code, run_id],
        )?;
        Ok(())
    }

    pub fn set_run_pid(&self, run_id: &str, pid: u32) -> Result<()> {
        self.conn.execute(
            "UPDATE runs SET pid = ?1 WHERE id = ?2",
            params![pid as i64, run_id],
        )?;
        Ok(())
    }

    pub fn insert_log(
        &self,
        run_id: &str,
        source: &str,
        stream: &str,
        level: LogLevel,
        message: &str,
        tags: &[String],
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let ts = time::now().to_rfc3339();
        let tags_json = serde_json::to_string(tags)?;
        self.conn.execute(
            "INSERT INTO log_events(id, run_id, source, ts, stream, level, message, tags) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, run_id, source, ts, stream, level.to_string(), message, tags_json],
        )?;
        self.conn.execute(
            "INSERT INTO log_fts(event_id, source, message) VALUES (?1, ?2, ?3)",
            params![id, source, message],
        )?;
        self.touch_source(source)?;
        Ok(id)
    }

    pub fn insert_error_block(
        &self,
        run_id: &str,
        source: &str,
        severity: Severity,
        title: &str,
        body: &str,
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let now = time::now().to_rfc3339();
        let fingerprint = fingerprint(title);
        self.conn.execute(
            "INSERT INTO error_blocks(id, run_id, source, start_ts, end_ts, severity, title, body, fingerprint) VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, ?8)",
            params![id, run_id, source, now, severity.to_string(), title, body, fingerprint],
        )?;
        self.touch_source(source)?;
        Ok(id)
    }

    fn touch_source(&self, source: &str) -> Result<()> {
        let now = time::now().to_rfc3339();
        self.conn.execute(
            "UPDATE sources SET last_seen_at = ?1 WHERE name = ?2",
            params![now, source],
        )?;
        Ok(())
    }

    pub fn list_sources(&self) -> Result<Vec<SourceInfo>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT
              s.name,
              s.first_seen_at,
              s.last_seen_at,
              s.run_count,
              s.last_command,
              s.last_cwd,
              r.started_at,
              r.ended_at,
              r.exit_code,
              r.pid,
              0 AS active_run_count
            FROM sources s
            LEFT JOIN runs r ON r.id = (
              SELECT id
              FROM runs
              WHERE source = s.name
              ORDER BY started_at DESC
              LIMIT 1
            )
            ORDER BY s.last_seen_at DESC
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            let last_ended_at: Option<String> = row.get(7)?;
            let last_pid: Option<i64> = row.get(9)?;
            let status = status_for_run(last_ended_at.as_deref(), last_pid);
            Ok(SourceInfo {
                name: row.get(0)?,
                first_seen_at: row.get(1)?,
                last_seen_at: row.get(2)?,
                run_count: row.get(3)?,
                last_command: row.get(4)?,
                last_cwd: row.get(5)?,
                active: status == "active",
                status: status.to_string(),
                active_run_count: if status == "active" { 1 } else { 0 },
                last_started_at: row.get(6)?,
                last_ended_at,
                last_exit_code: row.get(8)?,
                last_pid,
            })
        })?;
        collect_rows(rows)
    }

    pub fn logs_since(
        &self,
        since: DateTime<Utc>,
        source: Option<&str>,
        limit: usize,
        _latest_only: bool,
    ) -> Result<Vec<LogEvent>> {
        let active_run_ids = self.active_run_ids(source)?;
        if active_run_ids.is_empty() {
            return Ok(Vec::new());
        }

        let since = since.to_rfc3339();
        let placeholders = placeholders(active_run_ids.len());
        let sql = format!(
            "SELECT id, run_id, source, ts, stream, level, message, tags FROM log_events WHERE ts >= ? AND run_id IN ({placeholders}) ORDER BY ts DESC LIMIT ?"
        );
        let params = query_params(since, active_run_ids, limit);
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(params), map_log_event)?;
        collect_rows(rows)
    }

    pub fn error_blocks_since(
        &self,
        since: DateTime<Utc>,
        source: Option<&str>,
        limit: usize,
        _latest_only: bool,
    ) -> Result<Vec<ErrorBlock>> {
        let active_run_ids = self.active_run_ids(source)?;
        if active_run_ids.is_empty() {
            return Ok(Vec::new());
        }

        let since = since.to_rfc3339();
        let placeholders = placeholders(active_run_ids.len());
        let sql = format!(
            "SELECT id, run_id, source, start_ts, end_ts, severity, title, body, fingerprint FROM error_blocks WHERE start_ts >= ? AND run_id IN ({placeholders}) ORDER BY start_ts DESC LIMIT ?"
        );
        let params = query_params(since, active_run_ids, limit);
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(params), map_error_block)?;
        collect_rows(rows)
    }

    pub fn search_logs(
        &self,
        query: &str,
        since: DateTime<Utc>,
        source: Option<&str>,
        limit: usize,
        _latest_only: bool,
    ) -> Result<Vec<LogEvent>> {
        let active_run_ids = self.active_run_ids(source)?;
        if active_run_ids.is_empty() {
            return Ok(Vec::new());
        }

        let since = since.to_rfc3339();
        let placeholders = placeholders(active_run_ids.len());
        let sql = format!(
            r#"
            SELECT l.id, l.run_id, l.source, l.ts, l.stream, l.level, l.message, l.tags
            FROM log_fts f
            JOIN log_events l ON l.id = f.event_id
            WHERE log_fts MATCH ? AND l.ts >= ? AND l.run_id IN ({placeholders})
            ORDER BY l.ts DESC
            LIMIT ?
            "#
        );
        let mut params = vec![Value::Text(query.to_string()), Value::Text(since)];
        params.extend(active_run_ids.into_iter().map(Value::Text));
        params.push(Value::Integer(limit as i64));
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(params), map_log_event)?;
        collect_rows(rows)
    }

    pub fn logs_around_error(
        &self,
        error_id: &str,
        seconds: i64,
        limit: usize,
    ) -> Result<Vec<LogEvent>> {
        let error = self
            .error_block(error_id)?
            .with_context(|| format!("error block '{error_id}' not found"))?;
        if !self.run_is_active(&error.run_id)? {
            return Ok(Vec::new());
        }
        let ts = DateTime::parse_from_rfc3339(&error.start_ts)?.with_timezone(&Utc);
        let start = ts - chrono::Duration::seconds(seconds);
        let end = ts + chrono::Duration::seconds(seconds);

        let mut stmt = self.conn.prepare(
            "SELECT id, run_id, source, ts, stream, level, message, tags FROM log_events WHERE ts >= ?1 AND ts <= ?2 ORDER BY ts ASC LIMIT ?3",
        )?;
        let rows = stmt.query_map(
            params![start.to_rfc3339(), end.to_rfc3339(), limit as i64],
            map_log_event,
        )?;
        collect_rows(rows)
    }

    fn error_block(&self, error_id: &str) -> Result<Option<ErrorBlock>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, run_id, source, start_ts, end_ts, severity, title, body, fingerprint FROM error_blocks WHERE id = ?1 LIMIT 1",
        )?;
        Ok(stmt
            .query_row(params![error_id], map_error_block)
            .optional()?)
    }

    fn active_run_ids(&self, source: Option<&str>) -> Result<Vec<String>> {
        let mut ids = Vec::new();

        if let Some(source) = source {
            let mut stmt = self.conn.prepare(
                "SELECT id, pid FROM runs WHERE source = ?1 AND ended_at IS NULL ORDER BY started_at DESC",
            )?;
            let rows = stmt.query_map(params![source], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?))
            })?;
            for row in rows {
                let (id, pid) = row?;
                if pid.is_some_and(process_is_alive) {
                    ids.push(id);
                }
            }
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT id, pid FROM runs WHERE ended_at IS NULL ORDER BY started_at DESC",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?))
            })?;
            for row in rows {
                let (id, pid) = row?;
                if pid.is_some_and(process_is_alive) {
                    ids.push(id);
                }
            }
        }

        Ok(ids)
    }

    fn run_is_active(&self, run_id: &str) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare("SELECT ended_at, pid FROM runs WHERE id = ?1 LIMIT 1")?;
        let value = stmt
            .query_row(params![run_id], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                ))
            })
            .optional()?;
        Ok(matches!(value, Some((None, Some(pid))) if process_is_alive(pid)))
    }

    fn clear_source_runtime_data(&self, source: &str) -> Result<()> {
        let run_ids = self.run_ids_for_source(source)?;
        for run_id in run_ids {
            self.conn.execute(
                "DELETE FROM log_fts WHERE event_id IN (SELECT id FROM log_events WHERE run_id = ?1)",
                params![run_id],
            )?;
            self.conn.execute(
                "DELETE FROM error_blocks WHERE run_id = ?1",
                params![run_id],
            )?;
            self.conn
                .execute("DELETE FROM log_events WHERE run_id = ?1", params![run_id])?;
            self.conn
                .execute("DELETE FROM runs WHERE id = ?1", params![run_id])?;
        }
        Ok(())
    }

    pub fn create_checkpoint(&self, name: &str) -> Result<Checkpoint> {
        let checkpoint = Checkpoint {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            ts: time::now().to_rfc3339(),
        };
        self.conn.execute(
            "INSERT INTO checkpoints(id, name, ts) VALUES (?1, ?2, ?3)",
            params![checkpoint.id, checkpoint.name, checkpoint.ts],
        )?;
        Ok(checkpoint)
    }

    pub fn clear_all_runtime_data(&self, include_checkpoints: bool) -> Result<()> {
        self.conn.execute("DELETE FROM log_fts", [])?;
        self.conn.execute("DELETE FROM error_blocks", [])?;
        self.conn.execute("DELETE FROM log_events", [])?;
        self.conn.execute("DELETE FROM runs", [])?;
        self.conn.execute("DELETE FROM sources", [])?;
        if include_checkpoints {
            self.conn.execute("DELETE FROM checkpoints", [])?;
        }
        Ok(())
    }

    pub fn remove_source(&self, source: &str) -> Result<usize> {
        let run_ids = self.run_ids_for_source(source)?;
        if run_ids.is_empty() {
            return Ok(0);
        }

        for run_id in &run_ids {
            self.conn.execute(
                "DELETE FROM log_fts WHERE event_id IN (SELECT id FROM log_events WHERE run_id = ?1)",
                params![run_id],
            )?;
            self.conn.execute(
                "DELETE FROM error_blocks WHERE run_id = ?1",
                params![run_id],
            )?;
            self.conn
                .execute("DELETE FROM log_events WHERE run_id = ?1", params![run_id])?;
            self.conn
                .execute("DELETE FROM runs WHERE id = ?1", params![run_id])?;
        }
        self.conn
            .execute("DELETE FROM sources WHERE name = ?1", params![source])?;
        Ok(run_ids.len())
    }

    fn run_ids_for_source(&self, source: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM runs WHERE source = ?1 ORDER BY started_at")?;
        let rows = stmt.query_map(params![source], |row| row.get::<_, String>(0))?;
        collect_rows(rows)
    }

    pub fn find_checkpoint(&self, value: &str) -> Result<Checkpoint> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, ts FROM checkpoints WHERE id = ?1 OR name = ?1 ORDER BY ts DESC LIMIT 1",
        )?;
        stmt.query_row(params![value], |row| {
            Ok(Checkpoint {
                id: row.get(0)?,
                name: row.get(1)?,
                ts: row.get(2)?,
            })
        })
        .optional()?
        .with_context(|| format!("checkpoint '{value}' not found"))
    }
}

fn map_log_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<LogEvent> {
    let tags_json: String = row.get(7)?;
    Ok(LogEvent {
        id: row.get(0)?,
        run_id: row.get(1)?,
        source: row.get(2)?,
        ts: row.get(3)?,
        stream: row.get(4)?,
        level: row.get(5)?,
        message: row.get(6)?,
        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
    })
}

fn map_error_block(row: &rusqlite::Row<'_>) -> rusqlite::Result<ErrorBlock> {
    Ok(ErrorBlock {
        id: row.get(0)?,
        run_id: row.get(1)?,
        source: row.get(2)?,
        start_ts: row.get(3)?,
        end_ts: row.get(4)?,
        severity: row.get(5)?,
        title: row.get(6)?,
        body: row.get(7)?,
        fingerprint: row.get(8)?,
    })
}

fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>>,
) -> Result<Vec<T>> {
    let mut values = Vec::new();
    for row in rows {
        values.push(row?);
    }
    Ok(values)
}

fn fingerprint(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_digit() { '#' } else { ch })
        .collect::<String>()
        .to_lowercase()
}

fn is_package_manager_command(source: &str, _command: &str) -> bool {
    if !matches!(source, "npm" | "pnpm" | "yarn" | "bun") {
        return false;
    }
    true
}

fn source_from_cwd(cwd: &str) -> Option<String> {
    let cwd = std::path::Path::new(cwd);
    let package_json = cwd.join("package.json");
    if let Ok(raw) = std::fs::read_to_string(package_json) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(name) = value.get("name").and_then(|value| value.as_str()) {
                let short_name = name.rsplit('/').next().unwrap_or(name);
                let source = sanitize_source(short_name);
                if !source.is_empty() {
                    return Some(source);
                }
            }
        }
    }

    cwd.file_name()
        .and_then(|value| value.to_str())
        .map(sanitize_source)
        .filter(|value| !value.is_empty())
}

fn sanitize_source(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .collect::<String>()
        .to_lowercase()
}

fn placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(", ")
}

fn query_params(since: String, run_ids: Vec<String>, limit: usize) -> Vec<Value> {
    let mut params = vec![Value::Text(since)];
    params.extend(run_ids.into_iter().map(Value::Text));
    params.push(Value::Integer(limit as i64));
    params
}

fn status_for_run(ended_at: Option<&str>, pid: Option<i64>) -> &'static str {
    if ended_at.is_some() {
        return "stopped";
    }

    match pid {
        Some(pid) if process_is_alive(pid) => "active",
        Some(_) => "stale",
        None => "unknown",
    }
}

fn process_is_alive(pid: i64) -> bool {
    if pid <= 0 || pid > i32::MAX as i64 {
        return false;
    }

    let result = unsafe { libc::kill(pid as i32, 0) };
    if result == 0 {
        return true;
    }

    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(test)]
mod tests {
    use super::Store;
    use crate::detect::{LogLevel, Severity};
    use chrono::Utc;

    #[test]
    fn stores_redacted_command_and_error_context() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.db")).unwrap();
        let run_id = store
            .start_run(
                "api",
                "server --token=abc123 --db postgres://user:pass@localhost/db",
                "/tmp",
            )
            .unwrap();
        store.set_run_pid(&run_id, std::process::id()).unwrap();

        store
            .insert_log(
                &run_id,
                "api",
                "pty",
                LogLevel::Error,
                "Error: token=[REDACTED] failed",
                &[],
            )
            .unwrap();
        let error_id = store
            .insert_error_block(
                &run_id,
                "api",
                Severity::Error,
                "Error: token=[REDACTED] failed",
                "Error: token=[REDACTED] failed",
            )
            .unwrap();

        let sources = store.list_sources().unwrap();
        let command = sources[0].last_command.as_ref().unwrap();
        assert!(command.contains("token=[REDACTED]"));
        assert!(command.contains("postgres://[REDACTED]"));
        assert!(!command.contains("abc123"));
        assert!(!command.contains("user:pass"));
        assert_eq!(sources[0].last_cwd.as_deref(), Some("/tmp"));

        let events = store.logs_around_error(&error_id, 10, 10).unwrap();
        assert_eq!(events.len(), 1);

        let errors = store
            .error_blocks_since(Utc::now() - chrono::Duration::minutes(1), None, 10, true)
            .unwrap();
        assert_eq!(errors.len(), 1);

        let removed = store.remove_source("api").unwrap();
        assert_eq!(removed, 1);
        assert!(store.list_sources().unwrap().is_empty());
        assert!(
            store
                .error_blocks_since(Utc::now() - chrono::Duration::minutes(1), None, 10, true)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn new_run_clears_previous_runtime_data_for_source() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.db")).unwrap();

        let first_run = store.start_run("api", "pnpm run dev", "/tmp/api").unwrap();
        store.set_run_pid(&first_run, std::process::id()).unwrap();
        store
            .insert_log(&first_run, "api", "pty", LogLevel::Error, "Error: old", &[])
            .unwrap();
        store
            .insert_error_block(
                &first_run,
                "api",
                Severity::Error,
                "Error: old",
                "Error: old",
            )
            .unwrap();

        let second_run = store.start_run("api", "pnpm run dev", "/tmp/api").unwrap();
        store.set_run_pid(&second_run, std::process::id()).unwrap();
        store
            .insert_log(
                &second_run,
                "api",
                "pty",
                LogLevel::Error,
                "Error: new",
                &[],
            )
            .unwrap();
        store
            .insert_error_block(
                &second_run,
                "api",
                Severity::Error,
                "Error: new",
                "Error: new",
            )
            .unwrap();

        let errors = store
            .error_blocks_since(
                Utc::now() - chrono::Duration::minutes(1),
                Some("api"),
                10,
                true,
            )
            .unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].title, "Error: new");

        let old_search = store
            .search_logs(
                "old",
                Utc::now() - chrono::Duration::minutes(1),
                Some("api"),
                10,
                true,
            )
            .unwrap();
        assert!(old_search.is_empty());

        store.finish_run(&second_run, 0).unwrap();
        let stopped_errors = store
            .error_blocks_since(
                Utc::now() - chrono::Duration::minutes(1),
                Some("api"),
                10,
                true,
            )
            .unwrap();
        assert!(stopped_errors.is_empty());
    }
}
