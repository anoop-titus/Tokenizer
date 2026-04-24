use std::collections::HashSet;

use anyhow::Result;
use rusqlite::{params, Connection};

use crate::config::Config;

pub struct Db {
    conn: Connection,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ConversionRecord {
    pub id: i64,
    pub timestamp: String,
    pub file_path: String,
    pub action: String,
    pub original_bytes: i64,
    pub converted_bytes: i64,
    pub tokens_saved: i64,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct DailySavings {
    pub date: String,
    pub category: String,
    pub tokens_saved: i64,
    pub files_converted: i64,
}

impl Db {
    pub fn open() -> Result<Self> {
        let dir = Config::config_dir();
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("history.db");
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    fn init(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS conversions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                file_path TEXT NOT NULL,
                action TEXT NOT NULL,
                original_bytes INTEGER NOT NULL,
                converted_bytes INTEGER NOT NULL,
                tokens_saved INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS daily_savings (
                date TEXT NOT NULL,
                category TEXT NOT NULL,
                tokens_saved INTEGER NOT NULL,
                files_converted INTEGER NOT NULL,
                PRIMARY KEY (date, category)
            );
            CREATE TABLE IF NOT EXISTS optimized_files (
                file_path TEXT PRIMARY KEY,
                original_format TEXT NOT NULL,
                final_format TEXT NOT NULL,
                optimized_at TEXT NOT NULL,
                original_bytes INTEGER NOT NULL,
                final_bytes INTEGER NOT NULL
            );",
        )?;
        Ok(())
    }

    pub fn insert_conversion(
        &self,
        file_path: &str,
        action: &str,
        original_bytes: i64,
        converted_bytes: i64,
        tokens_saved: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO conversions (timestamp, file_path, action, original_bytes, converted_bytes, tokens_saved)
             VALUES (datetime('now'), ?1, ?2, ?3, ?4, ?5)",
            params![file_path, action, original_bytes, converted_bytes, tokens_saved],
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn get_conversions(&self, limit: usize) -> Result<Vec<ConversionRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, timestamp, file_path, action, original_bytes, converted_bytes, tokens_saved
             FROM conversions ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(ConversionRecord {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                file_path: row.get(2)?,
                action: row.get(3)?,
                original_bytes: row.get(4)?,
                converted_bytes: row.get(5)?,
                tokens_saved: row.get(6)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    #[allow(dead_code)]
    pub fn get_daily_savings(&self, days: usize) -> Result<Vec<DailySavings>> {
        let mut stmt = self.conn.prepare(
            "SELECT date, category, tokens_saved, files_converted
             FROM daily_savings ORDER BY date DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![days as i64], |row| {
            Ok(DailySavings {
                date: row.get(0)?,
                category: row.get(1)?,
                tokens_saved: row.get(2)?,
                files_converted: row.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn total_tokens_saved(&self) -> Result<i64> {
        let total: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(tokens_saved), 0) FROM conversions",
            [],
            |r| r.get(0),
        )?;
        Ok(total)
    }

    #[allow(dead_code)]
    pub fn is_optimized(&self, path: &str) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM optimized_files WHERE file_path = ?1",
            params![path],
            |r| r.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn mark_optimized(
        &self,
        path: &str,
        orig_fmt: &str,
        final_fmt: &str,
        orig_bytes: i64,
        final_bytes: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO optimized_files (file_path, original_format, final_format, optimized_at, original_bytes, final_bytes)
             VALUES (?1, ?2, ?3, datetime('now'), ?4, ?5)",
            params![path, orig_fmt, final_fmt, orig_bytes, final_bytes],
        )?;
        Ok(())
    }

    pub fn count_conversions(&self) -> Result<usize> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM conversions", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    pub fn get_optimized_paths(&self) -> Result<HashSet<String>> {
        let mut stmt = self.conn.prepare("SELECT file_path FROM optimized_files")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut set = HashSet::new();
        for r in rows {
            set.insert(r?);
        }
        Ok(set)
    }
}
