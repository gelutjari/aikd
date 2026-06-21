# AIKD Configuration Guide

Deep-dive into every configuration option.

## Configuration File Location

Default: `~/.aikd/config.yaml`

Override with: `aikd -c /path/to/config.yaml scan`

## Complete Configuration Reference

```yaml
version: 2.0.0  # Config format version (auto-migrated)

scan:
  # Directories to index (supports ~ expansion)
  include_paths:
    - "."
    - "~/projects/shared-libs"
  
  # Directories to skip
  exclude_paths:
    - "node_modules"
    - ".git"
    - "target"
    - "__pycache__"
    - ".venv"
    - "dist"
    - "build"
  
  # File extensions to index
  include_extensions:
    - "rs"
    - "ts"
    - "tsx"
    - "js"
    - "jsx"
    - "py"
    - "go"
    - "md"
    - "json"
    - "yaml"
    - "yml"
    - "toml"
    - "txt"
  
  # File extensions to skip
  exclude_extensions:
    - "lock"
    - "min.js"
    - "min.css"
  
  # Specific files to include/exclude
  include_files: []
  exclude_files:
    - ".env"
    - "*.bak"
    - "*.tmp"
    - "*.secret"
    - "*.key"
    - "*.pem"
  
  # Follow symbolic links (security risk)
  follow_symlinks: false

chunk:
  # Maximum tokens per chunk
  max_tokens: 1000
  
  # Minimum tokens per chunk
  min_tokens: 100
  
  # Overlap between chunks (for context preservation)
  overlap_tokens: 0

embedding:
  # Enable vector embeddings (required for hybrid search)
  enabled: true
  
  # Model name (currently only all-MiniLM-L6-v2 supported)
  model: all-MiniLM-L6-v2
  
  # Batch size (auto = detect based on RAM)
  # Options: "auto", "8", "16", "32", "64"
  batch_size: auto
  
  # Device for inference
  # Options: "cpu", "gpu" (requires CUDA)
  device: cpu
  
  # Number of compute threads
  compute_threads: 4

index:
  # SQLite database path
  db_path: ~/.aikd/aikd.db
  
  # Tantivy index path
  tantivy_path: ~/.aikd/tantivy_index
  
  # SQLite cache size in MB
  cache_size_mb: 512
  
  # Model storage path
  model_path: ~/.local/share/aikd/model

server:
  # REST API port
  rest_port: 9090
  
  # Authentication token (null = no auth)
  # Set this for production use!
  auth_token: null
  
  # CORS allowed origins
  # Use specific domains in production
  cors_origins:
    - "*"

filter:
  # Only index files with these words in filename
  filename_contains: []
  
  # Skip files with these words in filename
  filename_exclude: []
  
  # Only index files containing these strings
  content_contains: []
  
  # Skip files containing these strings
  content_exclude: []
  
  # Maximum file size in bytes (0 = unlimited)
  # 1MB = 1048576
  max_file_size: 1048576

resource:
  # Resource usage mode
  # Options: "Low", "Medium", "High", "Max", "Auto"
  mode: Auto
  
  # Maximum memory in MB (0 = unlimited)
  max_memory_mb: 0
```

## Resource Modes

| Mode | CPU | RAM | Batch Size | Best For |
|------|-----|-----|------------|----------|
| Low | 25% | 25% | 8 | Old laptops, shared servers |
| Medium | 50% | 50% | 16 | Development machines |
| High | 75% | 75% | 32 | Dedicated workstations |
| Max | 100% | 100% | 64 | CI/CD, batch processing |
| Auto | Auto | Auto | Auto | Let AIKD decide |

## Scenario Configurations

### Small Project (< 100 files)

```yaml
scan:
  include_paths: ["."]
  exclude_paths: [".git"]
chunk:
  max_tokens: 500
  min_tokens: 50
embedding:
  enabled: true
  batch_size: 16
```

### Monorepo (10,000+ files)

```yaml
scan:
  include_paths:
    - "packages/frontend/src"
    - "packages/backend/src"
    - "packages/shared"
  exclude_paths:
    - "node_modules"
    - ".git"
    - "dist"
    - "build"
    - "**/test/**"
    - "**/__tests__/**"
filter:
  max_file_size: 524288  # 512KB
chunk:
  max_tokens: 800
resource:
  mode: High
```

### Documentation Site

```yaml
scan:
  include_extensions: ["md", "mdx", "txt", "rst"]
  exclude_paths: [".git", "node_modules"]
filter:
  filename_exclude: ["CHANGELOG", "LICENSE"]
chunk:
  max_tokens: 1500
  min_tokens: 200
```

### Team Server

```yaml
server:
  rest_port: 9090
  auth_token: "your-secret-token-here"
  cors_origins:
    - "https://your-team-app.com"
    - "http://localhost:3000"
resource:
  mode: High
```

## Environment Variables

Override config values with environment variables:

```bash
export AIKD_REST_PORT=8080
export AIKD_AUTH_TOKEN="my-secret"
export AIKD_RESOURCE_MODE=Low
export AIKD_DB_PATH="/custom/path/aikd.db"
```

## Smart Config Generation

AIKD auto-detects project type and generates appropriate config:

```bash
aikd init  # Creates config based on project files
```

Detection rules:
- `Cargo.toml` → Rust project (include `.rs`, `Cargo.toml`)
- `package.json` → Node.js (include `.ts`, `.js`, `.json`)
- `pyproject.toml` → Python (include `.py`, `.toml`)
- `go.mod` → Go (include `.go`, `mod`, `sum`)
- `.git` → Git repo (exclude `.git`)
