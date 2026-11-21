use std::path::PathBuf;

#[cfg(target_os = "windows")]
pub fn default_aikd_dir() -> PathBuf {
    std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("C:\\"))
                .join("AppData")
                .join("Roaming")
        })
        .join("aikd")
}

#[cfg(not(target_os = "windows"))]
pub fn default_aikd_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".aikd")
}
/// Canonical DB/index path: forward slashes on every platform.
/// Windows APIs accept `/` for file access, so one spelling prevents
/// `C:\\x` vs `C:/x` duplicate rows when callers mix separators
/// (CLI `--path`, watcher events, config include_paths).
pub fn canonical_path(p: &std::path::Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}
/// Canonical project identity: the first include path in slash form,
/// so memory/events/atlas keys never fork on separator style.
pub fn canonical_project(include_paths: &[String]) -> String {
    include_paths.first().cloned().unwrap_or_default().replace('\\', "/")
}

pub fn detect_gpu() -> bool {
    #[cfg(target_os = "windows")]
    {
        if std::env::var("CUDA_PATH").is_ok() {
            return true;
        }
        std::process::Command::new("nvidia-smi")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[cfg(target_os = "linux")]
    {
        if std::path::Path::new("/proc/driver/nvidia").exists() {
            return true;
        }
        std::process::Command::new("nvidia-smi")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[cfg(target_os = "macos")]
    {
        false
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_aikd_dir_not_empty() {
        let dir = default_aikd_dir();
        assert!(!dir.as_os_str().is_empty());
    }

    #[test]
    fn test_detect_gpu_returns_bool() {
        let _gpu = detect_gpu();
    }

    #[test]
    fn canonical_path_unifies_separators() {
        assert_eq!(
            canonical_path(std::path::Path::new("C:\\tmp\\x\\main.rs")),
            "C:/tmp/x/main.rs"
        );
        assert_eq!(canonical_path(std::path::Path::new("/a/b.rs")), "/a/b.rs");
    }

    #[test]
    fn canonical_project_unifies_identity() {
        let v = vec!["C:\\proj".to_string()];
        assert_eq!(canonical_project(&v), "C:/proj");
        assert_eq!(canonical_project(&[]), "");
    }
}
