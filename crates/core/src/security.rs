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

/// Sanitize a path input string to prevent traversal attacks.
/// Checks for: null bytes, '..' traversal, symlinks, Windows UNC paths.
/// Returns a canonicalized PathBuf if safe.
pub fn sanitize_path_input(input: &str) -> Result<PathBuf, AikdError> {
    // Reject null bytes (used to bypass string checks)
    if input.contains('\0') {
        return Err(AikdError::PathTraversal("Path contains null bytes".into()));
    }

    // Reject Windows UNC paths (\\server\share) which can bypass checks
    if input.starts_with("\\\\") || input.starts_with("//") {
        return Err(AikdError::PathTraversal(
            "UNC/network paths are not allowed".into(),
        ));
    }

    let path = PathBuf::from(input);

    // Check each component for traversal attempts
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                return Err(AikdError::PathTraversal(
                    "Path contains '..' traversal".into(),
                ));
            }
            // Reject root paths on non-Windows (they shouldn't be user input)
            #[cfg(unix)]
            std::path::Component::RootDir => {
                return Err(AikdError::PathTraversal(
                    "Absolute root paths are not allowed".into(),
                ));
            }
            _ => {}
        }
    }

    // Attempt canonicalization to resolve symlinks and normalize path
    // This is the critical step that prevents symlink attacks
    if path.exists() {
        let canonical = path
            .canonicalize()
            .map_err(|e| AikdError::PathTraversal(format!("Failed to canonicalize path: {e}")))?;

        // Verify canonical path doesn't escape to sensitive directories
        let canonical_str = canonical.to_string_lossy().to_lowercase();
        let sensitive_dirs = [
            "/etc",
            "/proc",
            "/sys",
            "/dev", // Unix sensitive
            "c:\\windows",
            "c:\\program files", // Windows sensitive
        ];
        for sensitive in &sensitive_dirs {
            if canonical_str.starts_with(&sensitive.to_lowercase()) {
                return Err(AikdError::PathTraversal(format!(
                    "Path resolves to sensitive directory: {}",
                    canonical.display()
                )));
            }
        }

        return Ok(canonical);
    }

    // Path doesn't exist yet — return as-is (parent directory validation should be done separately)
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_rejects_null_bytes() {
        let result = sanitize_path_input("foo\0bar");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("null bytes"));
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
        assert!(result.unwrap_err().to_string().contains("'..' traversal"));
    }

    #[test]
    fn test_sanitize_rejects_embedded_traversal() {
        let result = sanitize_path_input("/home/user/../../../etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn test_sanitize_rejects_unc_paths() {
        let result = sanitize_path_input("\\\\server\\share\\file.txt");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("UNC/network"));
    }

    #[test]
    fn test_sanitize_rejects_double_slash_network() {
        let result = sanitize_path_input("//server/share/file.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_empty_roots_rejects() {
        let dir = std::env::temp_dir();
        let result = validate_scan_path(&dir, &[]);
        assert!(result.is_err());
    }
}
