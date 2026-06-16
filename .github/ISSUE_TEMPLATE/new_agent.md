---
name: Add AI Agent Support
about: Request support for a new AI agent (Aider, OpenHands, Zed, etc.)
title: "[AGENT] "
labels: agent-support
assignees: ''
---

## Agent Information
- **Agent Name:** [e.g., Aider, OpenHands, Zed]
- **Website:** [e.g., https://aider.chat]
- **MCP Support:** Yes / No / Partial

## Config File Location
Where does this agent store its MCP configuration?
- **Path:** [e.g., `~/.aider.conf.yml`]
- **Format:** [e.g., JSON, YAML, TOML]

## Config Format
What does the MCP server entry look like for this agent?
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

## Additional Context
Any other information about how this agent integrates with MCP servers.
