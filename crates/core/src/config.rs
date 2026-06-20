use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::resource::ResourceMode;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub scan: ScanConfig,
    #[serde(default)]
    pub chunk: ChunkConfig,
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    #[serde(default)]
    pub index: IndexConfig,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub filter: FilterConfig,
    #[serde(default)]
    pub resource: ResourceConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanConfig {
    #[serde(default = "default_paths")]
    pub include_paths: Vec<String>,
    #[serde(default)]
    pub exclude_paths: Vec<String>,
    #[serde(default = "default_extensions")]
    pub include_extensions: Vec<String>,
    #[serde(default)]
    pub exclude_extensions: Vec<String>,
    #[serde(default)]
    pub include_files: Vec<String>,
    #[serde(default)]
    pub exclude_files: Vec<String>,
    #[serde(default)]
    pub follow_symlinks: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkConfig {
    #[serde(default = "default_max_chunk_tokens")]
    pub max_tokens: usize,
    #[serde(default = "default_min_chunk_tokens")]
    pub min_tokens: usize,
    #[serde(default)]
    pub overlap_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_auto")]
    pub batch_size: String,
    #[serde(default = "default_device")]
    pub device: String,
    #[serde(default = "default_compute_threads")]
    pub compute_threads: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexConfig {
    #[serde(default = "default_db_path")]
    pub db_path: String,
    #[serde(default = "default_tantivy_path")]
    pub tantivy_path: String,
    #[serde(default = "default_cache_size_mb")]
    pub cache_size_mb: usize,
    #[serde(default = "default_model_path")]
    pub model_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_rest_port")]
    pub rest_port: u16,
    #[serde(default)]
    pub auth_token: Option<String>,
    #[serde(default = "default_cors_origins")]
    pub cors_origins: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterConfig {
    #[serde(default)]
    pub filename_contains: Vec<String>,
    #[serde(default)]
    pub filename_exclude: Vec<String>,
    #[serde(default)]
    pub content_contains: Vec<String>,
    #[serde(default)]
    pub content_exclude: Vec<String>,
    #[serde(default)]
    pub max_file_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceConfig {
    #[serde(default)]
    pub mode: ResourceMode,
}

fn default_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
fn default_paths() -> Vec<String> {
    vec![".".to_string()]
}
fn default_extensions() -> Vec<String> {
    vec!["md", "json", "yaml", "yml", "txt", "toml"]
        .into_iter()
        .map(String::from)
        .collect()
}
fn default_max_chunk_tokens() -> usize {
    1000
}
fn default_min_chunk_tokens() -> usize {
    100
}
fn default_auto() -> String {
    "auto".to_string()
}
fn default_model() -> String {
    "all-MiniLM-L6-v2".to_string()
}
fn default_device() -> String {
    "cpu".to_string()
}
fn default_compute_threads() -> usize {
    4
}
fn default_db_path() -> String {
    "~/.aikd/aikd.db".to_string()
}
fn default_tantivy_path() -> String {
    "~/.aikd/tantivy_index".to_string()
}
fn default_cache_size_mb() -> usize {
    512
}
fn default_model_path() -> String {
    // Model terpisah dari config/DB agar tidak terhapus saat reset
    crate::platform::default_aikd_dir()
        .join("model")
        .to_string_lossy()
        .to_string()
}
fn default_true() -> bool {
    true
}
fn default_rest_port() -> u16 {
    9090
}
fn default_cors_origins() -> Vec<String> {
    vec!["*".to_string()]
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            include_paths: default_paths(),
            exclude_paths: vec![
                "node_modules",
                ".git",
                "__pycache__",
                ".cache",
                "target",
                ".cargo",
                "dist",
                "build",
                ".next",
                ".venv",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            include_extensions: default_extensions(),
            exclude_extensions: Vec::new(),
            include_files: Vec::new(),
            exclude_files: vec![".env", "*.bak", "*.tmp", "*.secret"]
                .into_iter()
                .map(String::from)
                .collect(),
            follow_symlinks: false,
        }
    }
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            max_tokens: default_max_chunk_tokens(),
            min_tokens: default_min_chunk_tokens(),
            overlap_tokens: 0,
        }
    }
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            model: default_model(),
            batch_size: default_auto(),
            device: default_device(),
            compute_threads: default_compute_threads(),
        }
    }
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            db_path: default_db_path(),
            tantivy_path: default_tantivy_path(),
            cache_size_mb: default_cache_size_mb(),
            model_path: default_model_path(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            rest_port: default_rest_port(),
            auth_token: None,
            cors_origins: default_cors_origins(),
        }
    }
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            filename_contains: Vec::new(),
            filename_exclude: Vec::new(),
            content_contains: Vec::new(),
            content_exclude: Vec::new(),
            max_file_size: 1_048_576,
        }
    }
}

impl Default for ResourceConfig {
    fn default() -> Self {
        Self {
            mode: ResourceMode::Auto,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: default_version(),
            scan: ScanConfig::default(),
            chunk: ChunkConfig::default(),
            embedding: EmbeddingConfig::default(),
            index: IndexConfig::default(),
            server: ServerConfig::default(),
            filter: FilterConfig::default(),
            resource: ResourceConfig::default(),
        }
    }
}

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let expanded = shellexpand::tilde(path);
        let content = std::fs::read_to_string(expanded.as_ref())?;
        let config: Config = serde_yaml::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self, path: &str) -> Result<()> {
        let expanded = shellexpand::tilde(path);
        if let Some(parent) = Path::new(expanded.as_ref()).parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_yaml::to_string(self)?;
        std::fs::write(expanded.as_ref(), content)?;
        Ok(())
    }

    pub fn db_path(&self) -> PathBuf {
        PathBuf::from(shellexpand::tilde(&self.index.db_path).as_ref())
    }

    pub fn tantivy_path(&self) -> PathBuf {
        PathBuf::from(shellexpand::tilde(&self.index.tantivy_path).as_ref())
    }

    pub fn model_path(&self) -> PathBuf {
        PathBuf::from(shellexpand::tilde(&self.index.model_path).as_ref())
    }

    pub fn should_exclude_dir(&self, dir_name: &str) -> bool {
        self.scan.exclude_paths.iter().any(|d| d == dir_name)
    }

    pub fn should_exclude_file(&self, file_name: &str) -> bool {
        self.scan.exclude_files.iter().any(|f| {
            if let Some(ext) = f.strip_prefix("*.") {
                file_name.ends_with(&format!(".{ext}"))
            } else {
                f == file_name
            }
        })
    }

    pub fn matches_filename_filter(&self, file_name: &str) -> bool {
        if !self.filter.filename_contains.is_empty()
            && !self
                .filter
                .filename_contains
                .iter()
                .any(|f| file_name.contains(f.as_str()))
        {
            return false;
        }
        if self
            .filter
            .filename_exclude
            .iter()
            .any(|f| file_name.contains(f.as_str()))
        {
            return false;
        }
        true
    }

    pub fn matches_content_filter(&self, content: &str) -> bool {
        if !self.filter.content_contains.is_empty()
            && !self
                .filter
                .content_contains
                .iter()
                .any(|f| content.contains(f.as_str()))
        {
            return false;
        }
        if self
            .filter
            .content_exclude
            .iter()
            .any(|f| content.contains(f.as_str()))
        {
            return false;
        }
        true
    }

    pub fn check_file_size(&self, size: u64) -> bool {
        self.filter.max_file_size == 0 || size <= self.filter.max_file_size
    }

    pub fn max_chunk_tokens(&self) -> usize {
        self.chunk.max_tokens
    }

    pub fn min_chunk_tokens(&self) -> usize {
        self.chunk.min_tokens
    }

    /// Validate config values and return warnings for invalid settings.
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if self.server.rest_port < 1024 {
            warnings.push(format!(
                "rest_port {} is below 1024 (may require root privileges)",
                self.server.rest_port
            ));
        }

        if self.chunk.max_tokens < self.chunk.min_tokens {
            warnings.push(format!(
                "max_chunk_tokens ({}) < min_chunk_tokens ({})",
                self.chunk.max_tokens, self.chunk.min_tokens
            ));
        }

        for path in &self.scan.include_paths {
            let expanded = shellexpand::tilde(path);
            if !std::path::Path::new(expanded.as_ref()).exists() {
                warnings.push(format!("include_path does not exist: {path}"));
            }
        }

        warnings
    }
}

pub fn generate_smart_config(project_root: &Path) -> Config {
    let mut config = Config::default();

    if project_root.join(".git").exists() {
        config.scan.exclude_paths.extend(
            vec![
                ".git",
                "target",
                "node_modules",
                "dist",
                "build",
                ".next",
                ".venv",
                "__pycache__",
                ".cache",
            ]
            .into_iter()
            .map(String::from),
        );
    }

    if project_root.join("Cargo.toml").exists() {
        config.scan.include_extensions = vec!["rs", "md", "toml", "yaml", "yml", "txt"]
            .into_iter()
            .map(String::from)
            .collect();
        config
            .scan
            .exclude_paths
            .extend(vec!["target", ".cargo"].into_iter().map(String::from));
    } else if project_root.join("package.json").exists()
        || project_root.join("tsconfig.json").exists()
    {
        config.scan.include_extensions =
            vec!["ts", "tsx", "js", "jsx", "md", "json", "yaml", "yml"]
                .into_iter()
                .map(String::from)
                .collect();
        config.scan.exclude_paths.extend(
            vec!["node_modules", ".next", "dist", "build"]
                .into_iter()
                .map(String::from),
        );
    } else if project_root.join("pyproject.toml").exists()
        || project_root.join("requirements.txt").exists()
    {
        config.scan.include_extensions = vec![
            "py", "md", "yaml", "yml", "json", "txt", "toml", "cfg", "ini",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        config.scan.exclude_paths.extend(
            vec![
                ".venv",
                "venv",
                "__pycache__",
                ".mypy_cache",
                "dist",
                "build",
            ]
            .into_iter()
            .map(String::from),
        );
    } else if project_root.join("go.mod").exists() {
        config.scan.include_extensions = vec!["go", "md", "yaml", "yml", "json", "txt", "toml"]
            .into_iter()
            .map(String::from)
            .collect();
    }

    config.filter.max_file_size = 1_048_576;
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = Config::default();
        assert_eq!(cfg.version, env!("CARGO_PKG_VERSION"));
        assert!(cfg.scan.include_extensions.contains(&"md".to_string()));
        assert!(cfg.scan.exclude_paths.contains(&"node_modules".to_string()));
        assert_eq!(cfg.chunk.max_tokens, 1000);
        assert_eq!(cfg.server.rest_port, 9090);
    }

    #[test]
    fn test_smart_config_rust() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();
        let cfg = generate_smart_config(dir.path());
        assert!(cfg.scan.include_extensions.contains(&"rs".to_string()));
        assert!(cfg.scan.exclude_paths.contains(&"target".to_string()));
    }

    #[test]
    fn test_smart_config_node() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        let cfg = generate_smart_config(dir.path());
        assert!(cfg.scan.include_extensions.contains(&"ts".to_string()));
    }

    #[test]
    fn test_should_exclude_dir() {
        let cfg = Config::default();
        assert!(cfg.should_exclude_dir("node_modules"));
        assert!(cfg.should_exclude_dir(".git"));
        assert!(!cfg.should_exclude_dir("src"));
    }

    #[test]
    fn test_should_exclude_file() {
        let mut cfg = Config::default();
        assert!(!cfg.should_exclude_file("main.rs"));
        cfg.scan.exclude_files = vec![".env".to_string(), "*.bak".to_string()];
        assert!(cfg.should_exclude_file(".env"));
        assert!(cfg.should_exclude_file("backup.bak"));
        assert!(!cfg.should_exclude_file("main.rs"));
    }

    #[test]
    fn test_config_save_load() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.yaml");
        let path_str = config_path.to_str().unwrap();

        let cfg = Config::default();
        cfg.save(path_str).unwrap();

        let loaded = Config::load(path_str).unwrap();
        assert_eq!(loaded.version, cfg.version);
        assert_eq!(loaded.server.rest_port, 9090);
    }

    #[test]
    fn test_check_file_size() {
        let mut cfg = Config::default();
        assert!(cfg.check_file_size(100));
        cfg.filter.max_file_size = 1000;
        assert!(cfg.check_file_size(500));
        assert!(!cfg.check_file_size(2000));
    }
}
