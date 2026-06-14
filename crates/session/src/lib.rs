use anyhow::Result;
use rusqlite::Connection;
use aikd_core::{Session, Conversation};
use aikd_embedder::{self, MODEL_NAME, DIMENSIONS};

pub fn create_session(conn: &Connection, name: &str, project_path: &str) -> Result<Session> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO sessions (id, name, project_path, created_at, last_active) VALUES (?1,?2,?3,?4,?5)",
        rusqlite::params![id, name, project_path, now, now],
    )?;
    Ok(Session {
        id,
        name: name.to_string(),
        project_path: project_path.to_string(),
        created_at: now.clone(),
        last_active: now,
    })
}

pub fn get_or_create_session(conn: &Connection, project_path: &str) -> Result<Session> {
    let existing = conn.query_row(
        "SELECT id, name, project_path, created_at, last_active FROM sessions WHERE project_path = ?1 ORDER BY last_active DESC LIMIT 1",
        rusqlite::params![project_path],
        |row| Ok(Session {
            id: row.get(0)?,
            name: row.get(1)?,
            project_path: row.get(2)?,
            created_at: row.get(3)?,
            last_active: row.get(4)?,
        }),
    );

    match existing {
        Ok(mut session) => {
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute("UPDATE sessions SET last_active = ?1 WHERE id = ?2", rusqlite::params![now, session.id])?;
            session.last_active = now;
            Ok(session)
        }
        Err(_) => create_session(conn, &format!("Session for {}", project_path), project_path),
    }
}

pub fn list_sessions(conn: &Connection) -> Result<Vec<Session>> {
    let mut stmt = conn.prepare("SELECT id, name, project_path, created_at, last_active FROM sessions ORDER BY last_active DESC")?;
    let sessions = stmt.query_map([], |row| {
        Ok(Session {
            id: row.get(0)?,
            name: row.get(1)?,
            project_path: row.get(2)?,
            created_at: row.get(3)?,
            last_active: row.get(4)?,
        })
    })?.filter_map(|r| r.ok()).collect();
    Ok(sessions)
}

pub fn remember(
    conn: &Connection,
    session_id: &str,
    role: &str,
    content: &str,
    chunk_refs: &[String],
) -> Result<Conversation> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let tokens = aikd_core::estimate_tokens(content);
    let refs_json = serde_json::to_string(chunk_refs)?;

    conn.execute(
        "INSERT INTO conversations (id, session_id, role, content, tokens, chunk_refs, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        rusqlite::params![id, session_id, role, content, tokens as i64, refs_json, now],
    )?;

    // Update session last_active
    conn.execute("UPDATE sessions SET last_active = ?1 WHERE id = ?2", rusqlite::params![now, session_id])?;

    Ok(Conversation {
        id,
        session_id: session_id.to_string(),
        role: role.to_string(),
        content: content.to_string(),
        tokens,
        chunk_refs: chunk_refs.to_vec(),
        created_at: now,
    })
}

pub fn recall(
    conn: &Connection,
    session_id: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<Conversation>> {
    let query_lower = query.to_lowercase();
    let mut stmt = conn.prepare(
        "SELECT id, session_id, role, content, tokens, chunk_refs, created_at FROM conversations WHERE session_id = ?1 ORDER BY created_at DESC LIMIT 100"
    )?;

    let mut conversations: Vec<Conversation> = stmt.query_map(rusqlite::params![session_id], |row| {
        Ok(Conversation {
            id: row.get(0)?,
            session_id: row.get(1)?,
            role: row.get(2)?,
            content: row.get(3)?,
            tokens: row.get::<_, i64>(4)? as usize,
            chunk_refs: serde_json::from_str(&row.get::<_, String>(5).unwrap_or_default()).unwrap_or_default(),
            created_at: row.get(6)?,
        })
    })?.filter_map(|r| r.ok()).collect();

    // Simple keyword matching for recall
    if !query_lower.is_empty() {
        conversations.retain(|c| c.content.to_lowercase().contains(&query_lower));
    }

    conversations.truncate(limit);
    Ok(conversations)
}

pub fn embed_conversations(conn: &Connection, model_dir: &std::path::Path, session_id: &str) -> Result<usize> {
    let mut stmt = conn.prepare(
        "SELECT id, content FROM conversations WHERE session_id = ?1 AND id NOT IN (SELECT conversation_id FROM conversation_embeddings WHERE model = ?2)"
    )?;
    let rows: Vec<(String, String)> = stmt.query_map(rusqlite::params![session_id, MODEL_NAME], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?.filter_map(|r| r.ok()).collect();

    if rows.is_empty() {
        return Ok(0);
    }

    let mut model = aikd_embedder::create_model(model_dir)?;
    let texts: Vec<&str> = rows.iter().map(|(_, c)| c.as_str()).collect();
    let embeddings = model.embed(texts, None)?;

    let mut total = 0;
    for ((id, _), emb) in rows.iter().zip(embeddings.iter()) {
        conn.execute(
            "INSERT OR REPLACE INTO conversation_embeddings (conversation_id, model, dimensions, vector) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![id, MODEL_NAME, DIMENSIONS as i64, aikd_embedder::f32_to_bytes(emb)],
        )?;
        total += 1;
    }

    Ok(total)
}

pub fn get_session_stats(conn: &Connection) -> Result<(i64, i64, i64)> {
    let sc: i64 = conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;
    let cc: i64 = conn.query_row("SELECT COUNT(*) FROM conversations", [], |r| r.get(0))?;
    let ec: i64 = conn.query_row("SELECT COUNT(*) FROM conversation_embeddings", [], |r| r.get(0)).unwrap_or(0);
    Ok((sc, cc, ec))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use aikd_storage::schema;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        schema::run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn test_create_session() {
        let conn = setup_db();
        let session = create_session(&conn, "test", "/project").unwrap();
        assert_eq!(session.name, "test");
        assert!(!session.id.is_empty());
    }

    #[test]
    fn test_get_or_create_session() {
        let conn = setup_db();
        let s1 = get_or_create_session(&conn, "/project").unwrap();
        let s2 = get_or_create_session(&conn, "/project").unwrap();
        assert_eq!(s1.id, s2.id);
    }

    #[test]
    fn test_remember_and_recall() {
        let conn = setup_db();
        let session = create_session(&conn, "test", "/project").unwrap();

        remember(&conn, &session.id, "user", "Hello world", &[]).unwrap();
        remember(&conn, &session.id, "assistant", "Hi there!", &[]).unwrap();

        let convs = recall(&conn, &session.id, "hello", 10).unwrap();
        assert_eq!(convs.len(), 1);
        assert_eq!(convs[0].content, "Hello world");
    }

    #[test]
    fn test_list_sessions() {
        let conn = setup_db();
        create_session(&conn, "s1", "/p1").unwrap();
        create_session(&conn, "s2", "/p2").unwrap();

        let sessions = list_sessions(&conn).unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn test_session_stats() {
        let conn = setup_db();
        let session = create_session(&conn, "test", "/project").unwrap();
        remember(&conn, &session.id, "user", "Hello", &[]).unwrap();

        let (sc, cc, _ec) = get_session_stats(&conn).unwrap();
        assert_eq!(sc, 1);
        assert_eq!(cc, 1);
    }
}
