pub mod schema;

use std::path::Path;
use anyhow::Result;
use rusqlite::Connection;

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;")?;
        let db = Self { conn };
        schema::run_migrations(&db.conn)?;
        Ok(db)
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn begin_transaction(&self) -> Result<Transaction<'_>> {
        let tx = self.conn.unchecked_transaction()?;
        Ok(Transaction { tx })
    }
}

pub struct Transaction<'a> {
    tx: rusqlite::Transaction<'a>,
}

impl<'a> Transaction<'a> {
    pub fn conn(&self) -> &Connection {
        &self.tx
    }

    pub fn commit(self) -> Result<()> {
        self.tx.commit()?;
        Ok(())
    }
}

pub fn compute_blake3(path: &Path) -> Result<String> {
    let content = std::fs::read(path)?;
    let hash = blake3::hash(&content);
    Ok(hash.to_hex().to_string())
}
