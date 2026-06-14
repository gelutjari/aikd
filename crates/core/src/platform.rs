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
}
