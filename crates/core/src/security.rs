use crate::error::AikdError;
use std::path::{Path, PathBuf};

pub fn validate_scan_path(
    requested: &Path,
    allowed_roots: &[PathBuf],
) -> Result<PathBuf, AikdError> {
    let canonical = requested.canonicalize().map_err(AikdError::Io)?;

    let is_allowed = allowed_roots.iter().any(|root| {
        if let Ok(root_canonical) = root.canonicalize() {
            canonical.starts_with(&root_canonical)
        } else {
            false
        }
    });

    if allowed_roots.is_empty() {
        return Err(AikdError::PathTraversal(
            "No allowed roots configured; all paths rejected".into(),
        ));
    }

    if !is_allowed {
        return Err(AikdError::PathTraversal(format!(
            "Path '{}' is not within allowed directories: {:?}",
            canonical.display(),
            allowed_roots
        )));
    }

    Ok(canonical)
}

pub fn sanitize_path_input(input: &str) -> Result<PathBuf, AikdError> {
    if input.contains('\0') {
        return Err(AikdError::PathTraversal("Path contains null bytes".into()));
    }
    let path = PathBuf::from(input);
    for component in path.components() {
        if let std::path::Component::ParentDir = component {
            return Err(AikdError::PathTraversal(
                "Path contains '..' traversal".into(),
            ));
        }
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_rejects_null_bytes() {
        let result = sanitize_path_input("foo\0bar");
        assert!(result.is_err());
    }

    #[test]
    fn test_sanitize_accepts_normal_path() {
        let result = sanitize_path_input("/home/user/project");
        assert!(result.is_ok());
    }

    #[test]
    fn test_sanitize_accepts_relative_path() {
        let result = sanitize_path_input("./src/main.rs");
        assert!(result.is_ok());
    }

    #[test]
    fn test_sanitize_rejects_parent_traversal() {
        let result = sanitize_path_input("../../etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn test_sanitize_rejects_embedded_traversal() {
        let result = sanitize_path_input("/home/user/../../../etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_empty_roots_rejects() {
        let dir = std::env::temp_dir();
        let result = validate_scan_path(&dir, &[]);
        assert!(result.is_err());
    }
}
