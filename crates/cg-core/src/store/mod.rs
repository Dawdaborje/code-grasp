//! SQLite chunk store with FTS5 BM25 and hybrid RRF ranking.

mod hybrid;

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

pub use hybrid::reciprocal_rank_fusion;

use crate::chunker::Chunk;
use crate::error::CgError;

/// One row returned from search.
#[derive(Debug, Clone)]
pub struct ChunkHit {
    pub id: i64,
    pub score: f64,
    pub file_path: String,
    pub start_line: i64,
    pub end_line: i64,
    pub content: String,
}

/// SQLite persistence for chunks and full-text side index.
pub struct ChunkStore {
    conn: Connection,
}

impl ChunkStore {
    /// Open (or create) the database at `path` and ensure schema exists.
    pub fn open(path: &Path) -> Result<Self, CgError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(CgError::Io)?;
        }
        let conn = Connection::open(path).map_err(CgError::Database)?;
        let s = Self { conn };
        s.init_schema()?;
        Ok(s)
    }

    fn init_schema(&self) -> Result<(), CgError> {
        self.conn
            .execute_batch(
                r"
            CREATE TABLE IF NOT EXISTS index_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS files (
                path TEXT PRIMARY KEY,
                content_hash TEXT NOT NULL,
                indexed_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS chunks (
                id INTEGER PRIMARY KEY,
                file_path TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                start_byte INTEGER NOT NULL,
                end_byte INTEGER NOT NULL,
                language TEXT NOT NULL,
                content TEXT NOT NULL,
                content_hash TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_chunks_file ON chunks(file_path);
            CREATE INDEX IF NOT EXISTS idx_chunks_hash ON chunks(content_hash);

            CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
                content,
                content='chunks',
                content_rowid='id'
            );

            CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
                INSERT INTO chunks_fts(rowid, content) VALUES (new.id, new.content);
            END;

            CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
                INSERT INTO chunks_fts(chunks_fts, rowid) VALUES('delete', old.id);
            END;

            CREATE TRIGGER IF NOT EXISTS chunks_au AFTER UPDATE ON chunks BEGIN
                INSERT INTO chunks_fts(chunks_fts, rowid) VALUES('delete', old.id);
                INSERT INTO chunks_fts(rowid, content) VALUES (new.id, new.content);
            END;
            ",
            )
            .map_err(CgError::Database)?;
        Ok(())
    }

    /// Set a metadata key (e.g. embedding dimension).
    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), CgError> {
        self.conn
            .execute(
                "INSERT INTO index_meta(key, value) VALUES(?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(CgError::Database)?;
        Ok(())
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>, CgError> {
        let v = self
            .conn
            .query_row("SELECT value FROM index_meta WHERE key = ?1", [key], |r| {
                r.get::<_, String>(0)
            })
            .optional()
            .map_err(CgError::Database)?;
        Ok(v)
    }

    /// Delete all chunks (and FTS rows) for a file path.
    pub fn delete_chunks_for_file(&self, file_path: &str) -> Result<(), CgError> {
        self.conn
            .execute("DELETE FROM chunks WHERE file_path = ?1", [file_path])
            .map_err(CgError::Database)?;
        Ok(())
    }

    /// List chunk row ids for a file path.
    pub fn chunk_ids_for_file(&self, file_path: &str) -> Result<Vec<i64>, CgError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM chunks WHERE file_path = ?1")
            .map_err(CgError::Database)?;
        let rows = stmt
            .query_map([file_path], |r| r.get::<_, i64>(0))
            .map_err(CgError::Database)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(CgError::Database)?);
        }
        Ok(out)
    }

    /// Remove a row from the `files` table.
    pub fn delete_file_row(&self, path: &str) -> Result<(), CgError> {
        self.conn
            .execute("DELETE FROM files WHERE path = ?1", [path])
            .map_err(CgError::Database)?;
        Ok(())
    }

    /// Record file hash in `files` table.
    pub fn upsert_file(&self, path: &str, hash: &str, indexed_at: i64) -> Result<(), CgError> {
        self.conn
            .execute(
                "INSERT INTO files(path, content_hash, indexed_at) VALUES(?1, ?2, ?3)
                 ON CONFLICT(path) DO UPDATE SET content_hash = excluded.content_hash, indexed_at = excluded.indexed_at",
                params![path, hash, indexed_at],
            )
            .map_err(CgError::Database)?;
        Ok(())
    }

    /// Insert a chunk and return its row id.
    pub fn insert_chunk(&self, c: &Chunk) -> Result<i64, CgError> {
        let lang = format!("{:?}", c.language);
        self.conn
            .execute(
                "INSERT INTO chunks(file_path, start_line, end_line, start_byte, end_byte, language, content, content_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    c.file_path.to_string_lossy(),
                    c.start_line as i64,
                    c.end_line as i64,
                    c.start_byte as i64,
                    c.end_byte as i64,
                    lang,
                    c.content,
                    c.content_hash,
                ],
            )
            .map_err(CgError::Database)?;
        Ok(self.conn.last_insert_rowid())
    }

    /// BM25-ranked chunk ids for `query` (top `limit`).
    pub fn fts_search(&self, query: &str, limit: usize) -> Result<Vec<i64>, CgError> {
        let Some(pattern) = fts_query_pattern(query) else {
            return Ok(Vec::new());
        };
        let mut stmt = self
            .conn
            .prepare(
                "SELECT rowid
                 FROM chunks_fts
                 WHERE chunks_fts MATCH ?1
                 ORDER BY bm25(chunks_fts) ASC
                 LIMIT ?2",
            )
            .map_err(CgError::Database)?;
        let rows = stmt
            .query_map(params![pattern, limit as i64], |r| r.get::<_, i64>(0))
            .map_err(CgError::Database)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(CgError::Database)?);
        }
        Ok(out)
    }

    /// Fetch chunk rows by ids (preserves given order for convenience).
    pub fn fetch_chunks(&self, ids: &[i64]) -> Result<Vec<ChunkHit>, CgError> {
        let mut out = Vec::new();
        for &id in ids {
            let row = self
                .conn
                .query_row(
                    "SELECT id, file_path, start_line, end_line, content FROM chunks WHERE id = ?1",
                    [id],
                    |r| {
                        Ok(ChunkHit {
                            id: r.get(0)?,
                            score: 0.0,
                            file_path: r.get(1)?,
                            start_line: r.get(2)?,
                            end_line: r.get(3)?,
                            content: r.get(4)?,
                        })
                    },
                )
                .optional()
                .map_err(CgError::Database)?;
            if let Some(hit) = row {
                out.push(hit);
            }
        }
        Ok(out)
    }

    pub fn chunk_count(&self) -> Result<u64, CgError> {
        let n: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
            .map_err(CgError::Database)?;
        Ok(n)
    }

    pub fn file_count(&self) -> Result<u64, CgError> {
        let n: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
            .map_err(CgError::Database)?;
        Ok(n)
    }

    pub fn last_indexed(&self) -> Result<Option<i64>, CgError> {
        self.conn
            .query_row("SELECT MAX(indexed_at) FROM files", [], |r| {
                r.get::<_, Option<i64>>(0)
            })
            .map_err(CgError::Database)
    }

    /// Clear all tables (full reset).
    pub fn clear_all(&self) -> Result<(), CgError> {
        self.conn
            .execute_batch(
                "DELETE FROM chunks; DELETE FROM files; DELETE FROM index_meta;
                 INSERT INTO chunks_fts(chunks_fts) VALUES('rebuild');",
            )
            .map_err(CgError::Database)?;
        Ok(())
    }
}

fn fts_query_pattern(q: &str) -> Option<String> {
    let parts: Vec<String> = q
        .split_whitespace()
        .map(|t| {
            t.chars()
                .filter(|c| c.is_alphanumeric() || *c == '_')
                .collect::<String>()
        })
        .filter(|t| !t.is_empty())
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" OR "))
    }
}
