use serde_json::json;
use std::path::{Path, PathBuf};

pub struct AgentConfig {
    pub name: &'static str,
    pub config_path: fn() -> Option<PathBuf>,
    pub write_config: fn(&Path) -> Result<(), String>,
}

pub fn all_agents() -> Vec<AgentConfig> {
    vec![
        AgentConfig {
            name: "Claude Code",
            config_path: claude_code_path,
            write_config: write_claude_code,
        },
        AgentConfig {
            name: "Cursor",
            config_path: cursor_path,
            write_config: write_cursor,
        },
        AgentConfig {
            name: "Cline (VSCode)",
            config_path: cline_path,
            write_config: write_cline,
        },
        AgentConfig {
            name: "Continue (VSCode)",
            config_path: continue_path,
            write_config: write_continue,
        },
        AgentConfig {
            name: "Windsurf",
            config_path: windsurf_path,
            write_config: write_windsurf,
        },
        AgentConfig {
            name: "MiMoCode",
            config_path: mimocode_path,
            write_config: write_mimocode,
        },
    ]
}

pub fn detect_and_register(aikd_binary: &str) -> Vec<(&'static str, bool)> {
    let mut results = Vec::new();
    for agent in all_agents() {
        if let Some(_path) = (agent.config_path)() {
            match (agent.write_config)(Path::new(aikd_binary)) {
                Ok(()) => results.push((agent.name, true)),
                Err(_) => results.push((agent.name, false)),
            }
        }
    }
    results
}

// ─── Claude Code ───

fn claude_code_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".claude.json");
    Some(path)
}

fn write_claude_code(aikd_binary: &Path) -> Result<(), String> {
    let path = claude_code_path().ok_or("No home dir")?;
    let binary = aikd_binary.to_str().ok_or("Invalid path")?;

    let mut config: serde_json::Value = if path.exists() {
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or(json!({}))
    } else {
        json!({})
    };

    let mcp_servers = config.get("mcpServers").cloned().unwrap_or(json!({}));
    let mut servers = mcp_servers.as_object().cloned().unwrap_or_default();

    servers.insert(
        "aikd".into(),
        json!({
            "command": binary,
            "args": ["serve"],
        }),
    );

    config["mcpServers"] = json!(servers);

    let content = serde_json::to_string_pretty(&config).unwrap();
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

// ─── Cursor ───

fn cursor_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".cursor").join("mcp.json");
    if path.exists() || home.join(".cursor").exists() {
        Some(path)
    } else {
        None
    }
}

fn write_cursor(aikd_binary: &Path) -> Result<(), String> {
    let path = cursor_path().ok_or("Cursor not found")?;
    let binary = aikd_binary.to_str().ok_or("Invalid path")?;

    let mut config: serde_json::Value = if path.exists() {
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or(json!({}))
    } else {
        json!({})
    };

    let mcp_servers = config.get("mcpServers").cloned().unwrap_or(json!({}));
    let mut servers = mcp_servers.as_object().cloned().unwrap_or_default();

    servers.insert(
        "aikd".into(),
        json!({
            "command": binary,
            "args": ["serve"],
        }),
    );

    config["mcpServers"] = json!(servers);

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let content = serde_json::to_string_pretty(&config).unwrap();
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

// ─── Cline ───

fn cline_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    // Cline stores config in VSCode settings or its own config
    let path = home.join(".cline").join("mcp.json");
    if path.exists() || home.join(".cline").exists() {
        return Some(path);
    }
    // Also check VSCode global settings
    #[cfg(target_os = "windows")]
    {
        let appdata = PathBuf::from(std::env::var("APPDATA").ok()?);
        let path = appdata.join("Code").join("User").join("settings.json");
        if path.exists() {
            return Some(path);
        }
    }
    None
}

fn write_cline(aikd_binary: &Path) -> Result<(), String> {
    let path = cline_path().ok_or("Cline not found")?;
    let binary = aikd_binary.to_str().ok_or("Invalid path")?;

    // Cline uses VSCode settings format
    if path
        .file_name()
        .map(|f| f == "settings.json")
        .unwrap_or(false)
    {
        let mut config: serde_json::Value = if path.exists() {
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            serde_json::from_str(&content).unwrap_or(json!({}))
        } else {
            json!({})
        };
        config["cline.mcpServers"] = json!({
            "aikd": {
                "command": binary,
                "args": ["serve"],
            }
        });
        let content = serde_json::to_string_pretty(&config).unwrap();
        std::fs::write(&path, content).map_err(|e| e.to_string())
    } else {
        let config = json!({
            "mcpServers": {
                "aikd": {
                    "command": binary,
                    "args": ["serve"],
                }
            }
        });
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let content = serde_json::to_string_pretty(&config).unwrap();
        std::fs::write(&path, content).map_err(|e| e.to_string())
    }
}

// ─── Continue ───

fn continue_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".continue").join("config.json");
    if path.exists() || home.join(".continue").exists() {
        Some(path)
    } else {
        None
    }
}

fn write_continue(aikd_binary: &Path) -> Result<(), String> {
    let path = continue_path().ok_or("Continue not found")?;
    let binary = aikd_binary.to_str().ok_or("Invalid path")?;

    let mut config: serde_json::Value = if path.exists() {
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or(json!({}))
    } else {
        json!({})
    };

    // Continue uses "mcpServers" in its config
    let mcp_servers = config.get("mcpServers").cloned().unwrap_or(json!([]));
    let mut servers = mcp_servers.as_array().cloned().unwrap_or_default();

    // Check if aikd already exists
    let has_aikd = servers
        .iter()
        .any(|s| s.get("name").and_then(|n| n.as_str()) == Some("aikd"));

    if !has_aikd {
        servers.push(json!({
            "name": "aikd",
            "command": binary,
            "args": ["serve"],
        }));
        config["mcpServers"] = json!(servers);
        let content = serde_json::to_string_pretty(&config).unwrap();
        std::fs::write(&path, content).map_err(|e| e.to_string())?;
    }

    Ok(())
}

// ─── Windsurf ───

fn windsurf_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".windsurf").join("mcp.json");
    if path.exists() || home.join(".windsurf").exists() {
        Some(path)
    } else {
        None
    }
}

fn write_windsurf(aikd_binary: &Path) -> Result<(), String> {
    let path = windsurf_path().ok_or("Windsurf not found")?;
    let binary = aikd_binary.to_str().ok_or("Invalid path")?;

    let mut config: serde_json::Value = if path.exists() {
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or(json!({}))
    } else {
        json!({})
    };

    let mcp_servers = config.get("mcpServers").cloned().unwrap_or(json!({}));
    let mut servers = mcp_servers.as_object().cloned().unwrap_or_default();

    servers.insert(
        "aikd".into(),
        json!({
            "command": binary,
            "args": ["serve"],
        }),
    );

    config["mcpServers"] = json!(servers);

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let content = serde_json::to_string_pretty(&config).unwrap();
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

// ─── MiMoCode ───

fn mimocode_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".mcp.json");
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

fn write_mimocode(aikd_binary: &Path) -> Result<(), String> {
    let path = mimocode_path().ok_or("MiMoCode not found")?;
    let binary = aikd_binary.to_str().ok_or("Invalid path")?;

    let mut config: serde_json::Value = if path.exists() {
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or(json!({}))
    } else {
        json!({})
    };

    let mcp_servers = config.get("mcpServers").cloned().unwrap_or(json!({}));
    let mut servers = mcp_servers.as_object().cloned().unwrap_or_default();

    servers.insert(
        "aikd".into(),
        json!({
            "command": binary,
            "args": ["serve"],
        }),
    );

    config["mcpServers"] = json!(servers);

    let content = serde_json::to_string_pretty(&config).unwrap();
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_agents_returns_list() {
        let agents = all_agents();
        assert!(agents.len() >= 5);
    }

    #[test]
    fn test_agent_names() {
        let agents = all_agents();
        let names: Vec<&str> = agents.iter().map(|a| a.name).collect();
        assert!(names.contains(&"Claude Code"));
        assert!(names.contains(&"Cursor"));
        assert!(names.contains(&"MiMoCode"));
    }
}
