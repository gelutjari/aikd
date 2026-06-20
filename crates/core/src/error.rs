use thiserror::Error;

#[derive(Error, Debug)]
pub enum AikdError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Search index error: {0}")]
    SearchIndex(String),

    #[error("Embedding error: {0}")]
    Embedding(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Path traversal blocked: {0}")]
    PathTraversal(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, AikdError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display_database() {
        let err = AikdError::Config("test error".into());
        assert!(format!("{err}").contains("test error"));
    }

    #[test]
    fn test_error_display_path_traversal() {
        let err = AikdError::PathTraversal("blocked".into());
        assert!(format!("{err}").contains("blocked"));
    }

    #[test]
    fn test_error_display_session_not_found() {
        let err = AikdError::SessionNotFound("s1".into());
        assert!(format!("{err}").contains("s1"));
    }
}
