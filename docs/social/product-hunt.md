# Product Hunt Launch Copy

## Tagline
The Ultra-Fast Local Memory Layer for AI Coding Agents

## Description
Give Claude, Cursor, and Cline instant memory of your entire codebase. Written in Rust. Zero cloud dependency. Search 10,000 chunks in 0.21ms.

## First Comment

Hey Product Hunt! 👋

I'm excited to share **AIKD** — a tool that gives AI coding agents instant memory of your codebase.

### The Problem

Every time you start a new conversation with Claude, Cursor, or Cline, it forgets everything about your project. You have to re-explain your architecture, patterns, and conventions every single time.

### The Solution

AIKD indexes your code locally and provides instant search via MCP protocol. When you ask "how does authentication work?", AIKD instantly finds relevant code and provides context.

### Key Features

- **0.21ms search queries** — Ultra-fast hybrid search (BM25 + Vector)
- **100% local** — Your code never leaves your machine
- **Single binary** — No Python, no dependencies, just one install
- **MCP native** — Works with Claude, Cursor, Cline out of the box
- **7 built-in tools** — scan, query, embed, stats, remember, recall, status

### Benchmarks

| Operation | Time |
|-----------|------|
| Index 1,000 files | 144ms |
| BM25 Search | 0.21ms |
| Hybrid Search | 0.35ms |

### Try It

```bash
curl -sSf https://raw.githubusercontent.com/gelutjari/aikd/main/install.sh | bash
cd your-project && aikd init && aikd scan
aikd query "authentication"
```

Would love your feedback! What features would you like to see next?

## Maker Comment

I built AIKD because I was frustrated with constantly re-explaining my codebase to AI agents. 

The technical challenge was making search fast enough to be useful in real-time conversations. Rust was the perfect choice — 0.21ms per query across 10,000+ chunks.

The hybrid search approach (BM25 + Vector) gives the best of both worlds: fast keyword matching and semantic understanding.

Happy to answer any questions!
