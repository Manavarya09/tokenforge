use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::Path;

use crate::{SessionStats, TypeStats};

/// SQLite-backed full-fidelity store. Nothing is ever truly lost.
pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).context("failed to open database")?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS content_store (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                content_type TEXT NOT NULL,
                original_hash TEXT NOT NULL,
                original_content BLOB NOT NULL,
                compressed_content TEXT,
                original_tokens INTEGER NOT NULL,
                compressed_tokens INTEGER NOT NULL,
                compression_ratio REAL NOT NULL,
                metadata TEXT,
                created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S','now'))
            );

            CREATE TABLE IF NOT EXISTS compression_metrics (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                content_type TEXT NOT NULL,
                original_tokens INTEGER NOT NULL,
                compressed_tokens INTEGER NOT NULL,
                quality_score REAL,
                compression_level TEXT NOT NULL DEFAULT 'medium',
                created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S','now'))
            );

            CREATE TABLE IF NOT EXISTS project_profiles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_path TEXT NOT NULL,
                file_path TEXT NOT NULL,
                symbol_name TEXT,
                access_count INTEGER DEFAULT 1,
                last_accessed TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S','now')),
                avg_relevance_score REAL DEFAULT 0.5,
                UNIQUE(project_path, file_path, symbol_name)
            );

            CREATE TABLE IF NOT EXISTS mcp_tool_usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                call_count INTEGER DEFAULT 1,
                last_used_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S','now')),
                UNIQUE(session_id, tool_name)
            );

            CREATE TABLE IF NOT EXISTS session_config (
                session_id TEXT PRIMARY KEY,
                budget_total INTEGER,
                budget_conversation REAL,
                budget_tool_output REAL,
                budget_code_context REAL,
                budget_mcp_schema REAL,
                compression_level TEXT DEFAULT 'medium',
                created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S','now'))
            );

            CREATE INDEX IF NOT EXISTS idx_content_session ON content_store(session_id);
            CREATE INDEX IF NOT EXISTS idx_content_hash ON content_store(original_hash);
            CREATE INDEX IF NOT EXISTS idx_metrics_session ON compression_metrics(session_id);
            CREATE INDEX IF NOT EXISTS idx_profiles_project ON project_profiles(project_path);
            ",
        ).context("failed to initialize schema")?;

        Ok(())
    }

    /// Record a compression event.
    pub fn record_compression(
        &self,
        session_id: &str,
        content_type: &str,
        original_hash: &str,
        original: &str,
        compressed: &str,
        original_tokens: usize,
        compressed_tokens: usize,
    ) -> Result<()> {
        let ratio = if original_tokens > 0 {
            1.0 - (compressed_tokens as f64 / original_tokens as f64)
        } else {
            0.0
        };

        // Compress original with zstd for storage
        let compressed_blob = zstd::encode_all(original.as_bytes(), 3)
            .context("zstd compression failed")?;

        self.conn.execute(
            "INSERT INTO content_store (session_id, content_type, original_hash, original_content, compressed_content, original_tokens, compressed_tokens, compression_ratio)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                session_id,
                content_type,
                original_hash,
                compressed_blob,
                compressed,
                original_tokens as i64,
                compressed_tokens as i64,
                ratio,
            ],
        )?;

        self.conn.execute(
            "INSERT INTO compression_metrics (session_id, content_type, original_tokens, compressed_tokens)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                session_id,
                content_type,
                original_tokens as i64,
                compressed_tokens as i64,
            ],
        )?;

        Ok(())
    }

    /// Get the original uncompressed content by hash.
    pub fn get_original(&self, content_hash: &str) -> Result<String> {
        let blob: Vec<u8> = self.conn.query_row(
            "SELECT original_content FROM content_store WHERE original_hash = ?1 ORDER BY id DESC LIMIT 1",
            params![content_hash],
            |row| row.get(0),
        ).context("content not found for hash")?;

        let decompressed = zstd::decode_all(blob.as_slice())
            .context("zstd decompression failed")?;

        String::from_utf8(decompressed).context("content is not valid UTF-8")
    }

    /// Get session-level statistics.
    pub fn session_stats(&self, session_id: &str) -> Result<SessionStats> {
        let mut stmt = self.conn.prepare(
            "SELECT content_type, SUM(original_tokens), SUM(compressed_tokens), COUNT(*)
             FROM compression_metrics WHERE session_id = ?1
             GROUP BY content_type",
        )?;

        let mut by_type = Vec::new();
        let mut total_original = 0usize;
        let mut total_compressed = 0usize;
        let mut total_count = 0usize;

        let rows = stmt.query_map(params![session_id], |row| {
            let ct: String = row.get(0)?;
            let orig: i64 = row.get(1)?;
            let comp: i64 = row.get(2)?;
            let count: i64 = row.get(3)?;
            Ok((ct, orig as usize, comp as usize, count as usize))
        })?;

        for row in rows {
            let (ct, orig, comp, count) = row?;
            let ratio = if orig > 0 {
                1.0 - (comp as f64 / orig as f64)
            } else {
                0.0
            };
            by_type.push(TypeStats {
                content_type: ct,
                original_tokens: orig,
                compressed_tokens: comp,
                ratio,
                count,
            });
            total_original += orig;
            total_compressed += comp;
            total_count += count;
        }

        let overall_ratio = if total_original > 0 {
            1.0 - (total_compressed as f64 / total_original as f64)
        } else {
            0.0
        };

        Ok(SessionStats {
            session_id: session_id.to_string(),
            total_original_tokens: total_original,
            total_compressed_tokens: total_compressed,
            tokens_saved: total_original.saturating_sub(total_compressed),
            overall_ratio,
            compressions_count: total_count,
            by_type,
        })
    }

    /// Record file access for learning.
    pub fn record_file_access(&self, project_path: &str, file_path: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO project_profiles (project_path, file_path, symbol_name, access_count)
             VALUES (?1, ?2, NULL, 1)
             ON CONFLICT(project_path, file_path, symbol_name)
             DO UPDATE SET access_count = access_count + 1,
                           last_accessed = strftime('%Y-%m-%dT%H:%M:%S','now')",
            params![project_path, file_path],
        )?;
        Ok(())
    }

    /// Record MCP tool usage.
    pub fn record_tool_usage(&self, session_id: &str, tool_name: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO mcp_tool_usage (session_id, tool_name, call_count)
             VALUES (?1, ?2, 1)
             ON CONFLICT(session_id, tool_name)
             DO UPDATE SET call_count = call_count + 1,
                           last_used_at = strftime('%Y-%m-%dT%H:%M:%S','now')",
            params![session_id, tool_name],
        )?;
        Ok(())
    }

    /// Get top accessed files for a project.
    pub fn top_files(&self, project_path: &str, limit: usize) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT file_path, access_count FROM project_profiles
             WHERE project_path = ?1 AND symbol_name IS NULL
             ORDER BY access_count DESC LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![project_path, limit as i64], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;

        rows.collect::<Result<Vec<_>, _>>().context("failed to read profiles")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_compression_record() {
        let store = Store::open_in_memory().unwrap();
        store
            .record_compression("sess1", "command_output", "abc123", "hello world", "hello", 5, 3)
            .unwrap();

        let original = store.get_original("abc123").unwrap();
        assert_eq!(original, "hello world");
    }

    #[test]
    fn session_stats_work() {
        let store = Store::open_in_memory().unwrap();
        store
            .record_compression("sess1", "code", "h1", "long code here", "short", 100, 30)
            .unwrap();
        store
            .record_compression("sess1", "output", "h2", "big output", "small", 200, 50)
            .unwrap();

        let stats = store.session_stats("sess1").unwrap();
        assert_eq!(stats.total_original_tokens, 300);
        assert_eq!(stats.total_compressed_tokens, 80);
        assert_eq!(stats.tokens_saved, 220);
        assert_eq!(stats.compressions_count, 2);
    }
}
