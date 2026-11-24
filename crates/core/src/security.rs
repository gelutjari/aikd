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
        if let std::path::Component::ParentDir = component {
            return Err(AikdError::PathTraversal(
                "Path contains '..' traversal".into(),
            ));
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

/// Result of a deterministic secret-redaction pass over free text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RedactionResult {
    /// Scrubbed text (every matched secret replaced with `[REDACTED]`).
    pub text: String,
    /// Number of secret occurrences that were replaced.
    pub redacted_count: usize,
    /// Distinct secret families detected (e.g. `openai_key`, `bearer_token`).
    pub kinds: Vec<String>,
}

pub const REDACTED: &str = "[REDACTED]";

fn is_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

fn is_aws_char(c: char) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit()
}

fn is_slack_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-'
}

fn is_bearer_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '~' | '+' | '/' | '=' | '*')
}

fn is_value_char(c: char) -> bool {
    !c.is_whitespace()
        && !matches!(
            c,
            '"' | '\'' | '`' | '<' | '>' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | '&'
        )
}

/// Byte length of the leading run of `chars` starting at `s` that satisfy `pred`.
fn run_len(s: &str, pred: fn(char) -> bool) -> usize {
    s.chars()
        .take_while(|c| pred(*c))
        .map(|c| c.len_utf8())
        .sum()
}

fn record(kinds: &mut Vec<String>, kind: &str) {
    if !kinds.iter().any(|k| k == kind) {
        kinds.push(kind.to_string());
    }
}

/// Try to match one secret at the start of `rest`. `prev` is the preceding
/// character (or `None` at the start of input) so tokens embedded inside a
/// longer identifier are not clipped mid-word.
/// Returns `(consumed_bytes, kind, keep_prefix_bytes)`; the first
/// `keep_prefix_bytes` of the match (e.g. `api_key = "`) stay verbatim.
fn match_secret_at(rest: &str, prev: Option<char>) -> Option<(usize, &'static str, usize)> {
    // Never start a match in the middle of an identifier-like run.
    if let Some(p) = prev {
        if is_key_char(p) || p == '.' || p == '/' || p == '\\' {
            return None;
        }
    }

    // ---- provider-prefixed keys (longest prefixes first) ----
    for (prefix, kind) in [
        ("sk-ant-", "anthropic_key"),
        ("sk-proj-", "openai_project_key"),
        ("sk-", "openai_key"),
    ] {
        if let Some(tail) = rest.strip_prefix(prefix) {
            let n = run_len(tail, is_key_char);
            if n >= 16 {
                return Some((prefix.len() + n, kind, 0));
            }
        }
    }

    if let Some(tail) = rest.strip_prefix("AIza") {
        let n = run_len(tail, is_key_char);
        if n >= 20 {
            return Some((4 + n, "google_api_key", 0));
        }
    }

    // gh[pousr]_<token>
    if rest.len() >= 4 {
        let b = rest.as_bytes();
        if b[0] == b'g'
            && b[1] == b'h'
            && matches!(b[2], b'p' | b'o' | b'u' | b's' | b'r')
            && b[3] == b'_'
        {
            let n = run_len(&rest[4..], |c| c.is_ascii_alphanumeric() || c == '_');
            if n >= 20 {
                return Some((4 + n, "github_token", 0));
            }
        }
    }

    if let Some(tail) = rest.strip_prefix("AKIA") {
        let n = run_len(tail, is_aws_char);
        if n >= 16 {
            return Some((4 + n, "aws_access_key", 0));
        }
    }

    // xox[baprs]-<token>
    if rest.len() >= 5 {
        let b = rest.as_bytes();
        if b[0] == b'x'
            && b[1] == b'o'
            && b[2] == b'x'
            && matches!(b[3], b'b' | b'a' | b'p' | b'r' | b's')
            && b[4] == b'-'
        {
            let n = run_len(&rest[5..], is_slack_char);
            if n >= 12 {
                return Some((5 + n, "slack_token", 0));
            }
        }
    }

    // Bearer <token> — replace the credentials, keep the scheme.
    if rest.len() > 7 && rest.is_char_boundary(7) && rest[..7].eq_ignore_ascii_case("Bearer ") {
        let n = run_len(&rest[7..], is_bearer_char);
        if n >= 20 {
            return Some((7 + n, "bearer_token", 7));
        }
    }

    // ---- generic `name = value` credential assignments ----
    const ASSIGN_KEYS: [&str; 6] = ["api_key", "apikey", "token", "secret", "password", "passwd"];
    for key in ASSIGN_KEYS {
        if rest.len() < key.len() || !rest.is_char_boundary(key.len()) {
            continue;
        }
        if !rest[..key.len()].eq_ignore_ascii_case(key) {
            continue;
        }
        let after = &rest[key.len()..];
        let mut off = 0usize;
        for c in after.chars().take_while(|c| *c == ' ' || *c == '\t') {
            off += c.len_utf8();
        }
        let after_ws = &after[off..];
        let mut sep = 0usize;
        let mut sep_char = None;
        for c in after_ws.chars() {
            if c == ' ' || c == '\t' {
                sep += c.len_utf8();
                continue;
            }
            sep_char = Some(c);
            sep += c.len_utf8();
            break;
        }
        if !matches!(sep_char, Some('=') | Some(':')) {
            continue;
        }
        // Optional whitespace and an opening quote before the value.
        let mut val_off = 0usize;
        for c in after_ws[sep..].chars() {
            if c == ' ' || c == '\t' || c == '"' || c == '\'' {
                val_off += c.len_utf8();
            } else {
                break;
            }
        }
        let value_start = sep + val_off;
        let n = run_len(&after_ws[value_start..], is_value_char);
        if n >= 16 {
            let keep = key.len() + off + value_start;
            return Some((keep + n, "credential_assignment", keep));
        }
    }

    None
}

/// Deterministically scrub secrets from free text.
///
/// Covers OpenAI/Anthropic/DeepSeek-style `sk-` keys, Google API keys,
/// GitHub tokens, AWS access keys, Slack tokens, `Bearer` credentials and
/// generic `api_key=` / `token=` / `secret=` / `password=` assignments.
/// Every match becomes `[REDACTED]`; the result is byte-identical for the
/// same input, so callers can store it as a canonical, repeatable value.
pub fn redact_secrets(input: &str) -> RedactionResult {
    let mut out = String::with_capacity(input.len());
    let mut count = 0usize;
    let mut kinds: Vec<String> = Vec::new();
    let mut i = 0usize;
    let mut prev: Option<char> = None;

    while i < input.len() {
        let rest = &input[i..];
        if let Some((consumed, kind, keep)) = match_secret_at(rest, prev) {
            // Preserve the credential *name* (and any opening quote) but hide
            // its value: `api_key = "[REDACTED]"`, `Bearer [REDACTED]`.
            let keep = keep.min(rest.len());
            out.push_str(&rest[..keep]);
            out.push_str(REDACTED);
            count += 1;
            record(&mut kinds, kind);
            let consumed = consumed.min(rest.len());
            prev = rest[..consumed].chars().next_back();
            i += consumed;
        } else {
            let ch = rest.chars().next().expect("non-empty slice");
            out.push(ch);
            prev = Some(ch);
            i += ch.len_utf8();
        }
    }

    RedactionResult {
        text: out,
        redacted_count: count,
        kinds,
    }
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

    // ---- Secret Redaction Shield ----

    #[test]
    fn redact_leaves_clean_text_untouched() {
        let r = redact_secrets("hello world, no secrets here");
        assert_eq!(r.text, "hello world, no secrets here");
        assert_eq!(r.redacted_count, 0);
        assert!(r.kinds.is_empty());
    }

    #[test]
    fn redact_openai_and_anthropic_keys() {
        let input = "OPENAI=sk-abcdefghijklmnop1234 ANTHROPIC=sk-ant-api03-abcdefghijklmnopqrstu";
        let r = redact_secrets(input);
        assert!(!r.text.contains("abcdefghijklmnop"), "{}", r.text);
        assert_eq!(r.redacted_count, 2);
        assert!(r.kinds.contains(&"openai_key".to_string()));
        assert!(r.kinds.contains(&"anthropic_key".to_string()));
        assert!(r.text.contains(REDACTED));
    }

    #[test]
    fn redact_preserves_assignment_name() {
        let r = redact_secrets("api_key = \"supersecretvalue1234567890\" trailing");
        assert!(r.text.starts_with("api_key ="));
        assert!(r.text.contains(REDACTED));
        assert!(!r.text.contains("supersecretvalue"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn redact_short_values_are_not_touched() {
        let r = redact_secrets("token=short");
        assert_eq!(r.text, "token=short");
        assert_eq!(r.redacted_count, 0);
    }

    #[test]
    fn redact_bearer_github_aws_slack_google() {
        let input = "\
Authorization: Bearer abcdefghijklmnopqrstuvwxyz012345
ghp_abcdefghijklmnopqrstuvwxyz
AKIAIOSFODNN7EXAMPLE
xoxb-1234567890-abcdefgh
AIzaSyA1234567890abcdefghijklmnopqrstu";
        let r = redact_secrets(input);
        for leaked in [
            "abcdefghijklmnopqrstuvwxyz012345",
            "ghp_abcdefghijklmnopqrstuvwxyz",
            "AKIAIOSFODNN7EXAMPLE",
            "xoxb-1234567890",
            "AIzaSyA1234567890",
        ] {
            assert!(!r.text.contains(leaked), "leaked {leaked}: {}", r.text);
        }
        assert_eq!(r.redacted_count, 5);
        assert!(r.text.contains("Bearer [REDACTED]"));
    }

    #[test]
    fn redact_is_deterministic_and_idempotent() {
        let input = "key sk-proj-abcdefghijklmnopqrstuvwx and AKIAIOSFODNN7EXAMPLE";
        let a = redact_secrets(input);
        let b = redact_secrets(input);
        assert_eq!(a, b);
        let again = redact_secrets(&a.text);
        assert_eq!(again.redacted_count, 0);
        assert_eq!(again.text, a.text);
    }

    #[test]
    fn redact_does_not_clip_inside_identifiers() {
        // `task-` must not be mistaken for the tail of an sk- key.
        let r = redact_secrets("mytask-abcdefghijklmnopqrstuvwxyz");
        assert_eq!(r.redacted_count, 0);
    }
}
