use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub enum ResourceMode {
    Low,
    Medium,
    High,
    Max,
    #[default]
    Auto,
}

#[derive(Debug, Clone)]
pub struct ResourceProfile {
    pub cpu_cores: usize,
    pub total_ram_bytes: u64,
    pub has_gpu: bool,
    pub embedding_enabled: bool,
    pub batch_size: usize,
    pub parallelism: usize,
    pub hnsw_m: usize,
    pub hnsw_ef_construction: usize,
    pub hnsw_ef_search: usize,
    pub cache_size_mb: usize,
    pub tantivy_heap_mb: usize,
    pub watcher_debounce_ms: u64,
}

impl ResourceProfile {
    pub fn detect() -> Self {
        let cpu_cores = num_cpus::get();
        let total_ram = sysinfo::System::new_all().total_memory();
        let has_gpu = crate::platform::detect_gpu();

        Self::from_specs(cpu_cores, total_ram, has_gpu)
    }

    pub fn detect_with_mode(mode: &ResourceMode) -> Self {
        let mut profile = Self::detect();
        match mode {
            ResourceMode::Low => {
                profile.embedding_enabled = false;
                profile.batch_size = 1;
                profile.parallelism = 1;
                profile.hnsw_m = 4;
                profile.hnsw_ef_construction = 32;
                profile.hnsw_ef_search = 32;
                profile.cache_size_mb = 32;
                profile.tantivy_heap_mb = 10;
                profile.watcher_debounce_ms = 2000;
            }
            ResourceMode::Medium => {
                profile.embedding_enabled = true;
                profile.batch_size = 8;
                profile.parallelism = 2;
                profile.hnsw_m = 8;
                profile.hnsw_ef_construction = 64;
                profile.hnsw_ef_search = 64;
                profile.cache_size_mb = 128;
                profile.tantivy_heap_mb = 30;
                profile.watcher_debounce_ms = 1000;
            }
            ResourceMode::High => {
                profile.embedding_enabled = true;
                profile.batch_size = 32;
                profile.parallelism = 4;
                profile.hnsw_m = 16;
                profile.hnsw_ef_construction = 128;
                profile.hnsw_ef_search = 64;
                profile.cache_size_mb = 512;
                profile.tantivy_heap_mb = 50;
                profile.watcher_debounce_ms = 500;
            }
            ResourceMode::Max => {
                profile.embedding_enabled = true;
                profile.batch_size = 64;
                profile.parallelism = 8;
                profile.hnsw_m = 32;
                profile.hnsw_ef_construction = 200;
                profile.hnsw_ef_search = 128;
                profile.cache_size_mb = 1024;
                profile.tantivy_heap_mb = 100;
                profile.watcher_debounce_ms = 250;
            }
            ResourceMode::Auto => {} // keep detected values
        }
        profile
    }

    fn from_specs(cpu_cores: usize, total_ram: u64, has_gpu: bool) -> Self {
        let ram_gb = total_ram as f64 / (1024.0 * 1024.0 * 1024.0);

        if has_gpu {
            return Self {
                cpu_cores,
                total_ram_bytes: total_ram,
                has_gpu,
                embedding_enabled: true,
                batch_size: 256,
                parallelism: cpu_cores.min(16),
                hnsw_m: 64,
                hnsw_ef_construction: 256,
                hnsw_ef_search: 128,
                cache_size_mb: 1024,
                tantivy_heap_mb: 100,
                watcher_debounce_ms: 100,
            };
        }

        if ram_gb < 2.0 {
            Self {
                cpu_cores,
                total_ram_bytes: total_ram,
                has_gpu,
                embedding_enabled: false,
                batch_size: 1,
                parallelism: 1,
                hnsw_m: 4,
                hnsw_ef_construction: 32,
                hnsw_ef_search: 32,
                cache_size_mb: 32,
                tantivy_heap_mb: 10,
                watcher_debounce_ms: 2000,
            }
        } else if ram_gb < 8.0 || cpu_cores <= 4 {
            Self {
                cpu_cores,
                total_ram_bytes: total_ram,
                has_gpu,
                embedding_enabled: true,
                batch_size: 8,
                parallelism: 2,
                hnsw_m: 8,
                hnsw_ef_construction: 64,
                hnsw_ef_search: 64,
                cache_size_mb: 128,
                tantivy_heap_mb: 30,
                watcher_debounce_ms: 1000,
            }
        } else if ram_gb < 16.0 || cpu_cores <= 8 {
            Self {
                cpu_cores,
                total_ram_bytes: total_ram,
                has_gpu,
                embedding_enabled: true,
                batch_size: 32,
                parallelism: 4,
                hnsw_m: 16,
                hnsw_ef_construction: 128,
                hnsw_ef_search: 64,
                cache_size_mb: 512,
                tantivy_heap_mb: 50,
                watcher_debounce_ms: 500,
            }
        } else {
            Self {
                cpu_cores,
                total_ram_bytes: total_ram,
                has_gpu,
                embedding_enabled: true,
                batch_size: 64,
                parallelism: 8,
                hnsw_m: 32,
                hnsw_ef_construction: 200,
                hnsw_ef_search: 128,
                cache_size_mb: 1024,
                tantivy_heap_mb: 100,
                watcher_debounce_ms: 250,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_profile_detect() {
        let profile = ResourceProfile::detect();
        assert!(profile.cpu_cores > 0);
        assert!(profile.total_ram_bytes > 0);
    }

    #[test]
    fn test_resource_mode_low() {
        let profile = ResourceProfile::detect_with_mode(&ResourceMode::Low);
        assert!(!profile.embedding_enabled);
        assert_eq!(profile.batch_size, 1);
        assert_eq!(profile.parallelism, 1);
    }

    #[test]
    fn test_resource_mode_max() {
        let profile = ResourceProfile::detect_with_mode(&ResourceMode::Max);
        assert!(profile.embedding_enabled);
        assert_eq!(profile.batch_size, 64);
        assert_eq!(profile.hnsw_m, 32);
    }

    #[test]
    fn test_from_specs_low_ram() {
        let profile = ResourceProfile::from_specs(2, 1024 * 1024 * 1024, false); // 1GB
        assert!(!profile.embedding_enabled);
        assert_eq!(profile.parallelism, 1);
    }

    #[test]
    fn test_from_specs_high_end() {
        let profile = ResourceProfile::from_specs(16, 32 * 1024 * 1024 * 1024, false); // 32GB, 16 cores
        assert!(profile.embedding_enabled);
        assert_eq!(profile.batch_size, 64);
    }

    #[test]
    fn test_from_specs_gpu() {
        let profile = ResourceProfile::from_specs(8, 16 * 1024 * 1024 * 1024, true);
        assert!(profile.embedding_enabled);
        assert_eq!(profile.batch_size, 256);
        assert_eq!(profile.watcher_debounce_ms, 100);
    }
}
