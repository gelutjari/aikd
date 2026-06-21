# AIKD Quick Start Guide

Get up and running with AIKD in 5 minutes.

## Prerequisites

- **OS**: Windows 10+, macOS 12+, or Linux (Ubuntu 20.04+)
- **RAM**: 4GB minimum, 8GB recommended
- **Disk**: 200MB for binary + model

## Step 1: Install AIKD

**Linux/macOS:**
```bash
curl -sSf https://raw.githubusercontent.com/gelutjari/aikd/main/install.sh | bash
```

**Windows (PowerShell):**
```powershell
irm https://raw.githubusercontent.com/gelutjari/aikd/main/install.ps1 | iex
```

**From source:**
```bash
cargo install aikd
```

## Step 2: Initialize Your Project

```bash
cd your-project
aikd init
```

This creates `~/.aikd/config.yaml` with smart defaults based on your project type.

## Step 3: Scan Your Codebase

```bash
aikd scan
```

Output:
```
[aikd] Discovering files... found 847 files
[aikd] Checking for changes... 847 to index
[aikd] ████████████████████████████████████████ 847/847 files
[aikd] Storing 12,453 chunks from 847 files...
[aikd] Updating search index... done

Indexed 847 files, 12,453 chunks in 1.2s
```

## Step 4: Search

```bash
# Basic keyword search
aikd query "authentication"

# Semantic search (requires embeddings)
aikd embed                    # Generate embeddings first
aikd query "how login works" --hybrid

# JSON output for scripting
aikd query "database" --json --limit 5
```

## Step 5: Connect to AI Agent

### Claude Code / Cursor / Cline

AIKD auto-registers via MCP. Just restart your AI agent.

Verify MCP config exists:
```bash
cat ~/.aikd/mcp.json
```

Should contain:
```json
{
  "mcpServers": {
    "aikd": {
      "command": "aikd",
      "args": ["serve"]
    }
  }
}
```

### REST API

```bash
# Start daemon
aikd daemon

# Query via HTTP
curl "http://localhost:9090/api/query?q=authentication&limit=5"
```

## What's Next?

- [Configuration Guide](CONFIGURATION.md) — Customize AIKD for your needs
- [Architecture](ARCHITECTURE.md) — Understand how AIKD works
- [Recipes](RECIPE.md) — Common use cases and patterns
