# Twitter/X Thread

## Tweet 1
🚀 Just open-sourced AIKD — a Rust tool that gives AI coding agents instant memory of your codebase.

0.21ms search queries. 100% local. Single binary.

Here's what I learned building it 🧵👇

## Tweet 2
The problem: Every time you start a new conversation with Claude/Cursor/Cline, it forgets everything about your project.

You re-explain your architecture, patterns, conventions... every single time.

## Tweet 3
The solution: AIKD indexes your code locally and provides instant search via MCP protocol.

Technical stack:
• BM25 (Tantivy) for keyword search
• Vector embeddings (ONNX) for semantic search
• SQLite for storage
• Rust for speed

## Tweet 4
Why Rust?

• 0.21ms per query (Python RAG: 500ms+)
• 33MB single binary (Python: 2GB+ dependencies)
• 27% RAM usage (Python: 50%+)
• Zero cloud dependency

## Tweet 5
Try it:

```
curl -sSf https://raw.githubusercontent.com/gelutjari/aikd/main/install.sh | bash
cd your-project && aikd init && aikd scan
aikd query "authentication" --hybrid
```

GitHub: https://github.com/gelutjari/aikd

⭐ Star if you find it useful!

#Rust #AI #OpenSource #DeveloperTools
