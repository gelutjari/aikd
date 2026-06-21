# AIKD Recipes

Common use cases and patterns.

## Table of Contents

1. [CI/CD Integration](#cicd-integration)
2. [Team Shared Server](#team-shared-server)
3. [VS Code Extension](#vs-code-extension)
4. [Docker Deployment](#docker-deployment)
5. [Multi-Project Setup](#multi-project-setup)
6. [Custom Agent Integration](#custom-agent-integration)

---

## CI/CD Integration

### GitHub Actions

```yaml
# .github/workflows/ai-review.yml
name: AI Code Review
on: [pull_request]

jobs:
  review:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      
      - name: Install AIKD
        run: curl -sSf https://raw.githubusercontent.com/gelutjari/aikd/main/install.sh | bash
      
      - name: Index codebase
        run: aikd scan
      
      - name: Query for patterns
        run: |
          aikd query "error handling" --json > error-patterns.json
          aikd query "security" --json > security-patterns.json
      
      - name: Upload artifacts
        uses: actions/upload-artifact@v4
        with:
          name: ai-analysis
          path: *.json
```

### Pre-commit Hook

```bash
#!/bin/bash
# .git/hooks/pre-commit

# Re-index changed files
aikd scan

# Check for security patterns
RESULTS=$(aikd query "password,secret,api_key" --json)
if [ "$(echo $RESULTS | jq length)" -gt 0 ]; then
    echo "⚠️  Potential security patterns found:"
    echo $RESULTS | jq '.[].file_path'
    exit 1
fi
```

---

## Team Shared Server

### Setup Server

```bash
# On server machine
aikd init
aikd scan

# Start as daemon with auth
export AIKD_AUTH_TOKEN="team-secret-token"
aikd daemon
```

### Docker Compose

```yaml
version: '3.8'
services:
  aikd:
    image: ghcr.io/gelutjari/aikd:latest
    ports:
      - "9090:9090"
    volumes:
      - ./:/workspace
      - aikd-data:/root/.aikd
    environment:
      - AIKD_AUTH_TOKEN=${AIKD_AUTH_TOKEN}
      - AIKD_RESOURCE_MODE=High
    restart: unless-stopped

volumes:
  aikd-data:
```

### Client Usage

```bash
# Query team server
curl -H "Authorization: Bearer team-secret-token" \
  "http://team-server:9090/api/query?q=authentication&limit=5"
```

---

## VS Code Extension

Create `.vscode/settings.json`:

```json
{
  "aikd.serverUrl": "http://localhost:9090",
  "aikd.autoIndex": true,
  "aikd.searchOnSave": true
}
```

---

## Docker Deployment

### Simple Dockerfile

```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/aikd /usr/local/bin/
EXPOSE 9090
CMD ["aikd", "daemon", "foreground"]
```

### Build and Run

```bash
docker build -t aikd .
docker run -p 9090:9090 -v $(pwd):/workspace aikd
```

---

## Multi-Project Setup

### Index Multiple Projects

```yaml
# ~/.aikd/config.yaml
scan:
  include_paths:
    - ~/projects/frontend
    - ~/projects/backend
    - ~/projects/shared-libs
  exclude_paths:
    - "node_modules"
    - ".git"
    - "target"
```

### Per-Project Config

```bash
# Project-specific config
aikd -c ./aikd.yaml scan
aikd -c ./aikd.yaml query "login"
```

---

## Custom Agent Integration

### Python Agent

```python
import requests

def query_aikd(query: str, limit: int = 5) -> list:
    response = requests.get(
        "http://localhost:9090/api/query",
        params={"q": query, "limit": limit}
    )
    return response.json()["data"]

# Usage
results = query_aikd("authentication flow")
for r in results:
    print(f"{r['file_path']}:{r['line_start']}-{r['line_end']}")
    print(f"  {r['content'][:200]}")
```

### Node.js Agent

```javascript
async function queryAIKD(query, limit = 5) {
  const response = await fetch(
    `http://localhost:9090/api/query?q=${encodeURIComponent(query)}&limit=${limit}`
  );
  const data = await response.json();
  return data.data;
}

// Usage
const results = await queryAIKD("error handling");
results.forEach(r => {
  console.log(`${r.file_path}:${r.line_start}-${r.line_end}`);
  console.log(`  ${r.content.substring(0, 200)}`);
});
```

### MCP Protocol (Direct)

```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "query",
    "arguments": {
      "query": "authentication",
      "limit": 5,
      "hybrid": true
    }
  },
  "id": 1
}
```
