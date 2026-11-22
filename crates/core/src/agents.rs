use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub struct AgentConfig {
    pub name: &'static str,
    pub kind: &'static str,
    pub config_path: fn() -> Option<PathBuf>,
    pub write_config: fn(&Path) -> Result<(), String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentStatus {
    pub name: &'static str,
    pub kind: &'static str,
    pub config_path: String,
    pub detected: bool,
    pub registered: bool,
}

pub fn all_agents() -> Vec<AgentConfig> {
    vec![
        AgentConfig {
            name: "Claude Code",
            kind: "CLI",
            config_path: claude_code_path,
            write_config: write_claude_code,
        },
        AgentConfig {
            name: "Claude Code (.claude/mcp.json)",
            kind: "CLI",
            config_path: claude_dir_mcp_path,
            write_config: write_claude_dir_mcp,
        },
        AgentConfig {
            name: "Claude Desktop",
            kind: "GUI",
            config_path: claude_desktop_path,
            write_config: write_claude_desktop,
        },
        AgentConfig {
            name: "Antigravity",
            kind: "CLI & IDE",
            config_path: antigravity_path,
            write_config: write_antigravity,
        },
        AgentConfig {
            name: "Antigravity IDE",
            kind: "GUI / IDE",
            config_path: antigravity_ide_path,
            write_config: write_antigravity_ide,
        },
        AgentConfig {
            name: "Gemini CLI",
            kind: "CLI",
            config_path: gemini_cli_path,
            write_config: write_gemini_cli,
        },
        AgentConfig {
            name: "OpenAI Codex CLI",
            kind: "CLI",
            config_path: codex_cli_path,
            write_config: write_codex_cli,
        },
        AgentConfig {
            name: "OpenCode CLI",
            kind: "CLI",
            config_path: opencode_cli_path,
            write_config: write_opencode_cli,
        },
        AgentConfig {
            name: "Pi / OMP Agent CLI",
            kind: "CLI",
            config_path: pi_agent_path,
            write_config: write_pi_agent,
        },
        AgentConfig {
            name: "Qwen Code CLI",
            kind: "CLI",
            config_path: qwen_cli_path,
            write_config: write_qwen_cli,
        },
        AgentConfig {
            name: "Hermes Agent CLI",
            kind: "CLI",
            config_path: hermes_cli_path,
            write_config: write_hermes_cli,
        },
        AgentConfig {
            name: "Qoder",
            kind: "GUI / IDE",
            config_path: qoder_path,
            write_config: write_qoder,
        },
        AgentConfig {
            name: "Cursor",
            kind: "GUI / IDE",
            config_path: cursor_path,
            write_config: write_cursor,
        },
        AgentConfig {
            name: "Cline (VSCode)",
            kind: "GUI / IDE",
            config_path: cline_path,
            write_config: write_cline,
        },
        AgentConfig {
            name: "Continue (VSCode)",
            kind: "GUI / IDE",
            config_path: continue_path,
            write_config: write_continue,
        },
        AgentConfig {
            name: "Windsurf",
            kind: "GUI / IDE",
            config_path: windsurf_path,
            write_config: write_windsurf,
        },
        AgentConfig {
            name: "MiMoCode",
            kind: "CLI",
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

pub fn detect_agent_statuses() -> Vec<AgentStatus> {
    all_agents()
        .into_iter()
        .map(|agent| {
            if let Some(p) = (agent.config_path)() {
                let path_str = crate::platform::canonical_path(&p);
                let content = std::fs::read_to_string(&p).unwrap_or_default();
                let registered = content.contains("\"aikd\"")
                    || content.contains("aikd:")
                    || content.contains("[mcp_servers.aikd]");
                AgentStatus {
                    name: agent.name,
                    kind: agent.kind,
                    config_path: path_str,
                    detected: true,
                    registered,
                }
            } else {
                AgentStatus {
                    name: agent.name,
                    kind: agent.kind,
                    config_path: String::new(),
                    detected: false,
                    registered: false,
                }
            }
        })
        .collect()
}

fn norm_bin(aikd_binary: &Path) -> Result<String, String> {
    let s = aikd_binary.to_str().ok_or("Invalid binary path")?;
    Ok(s.replace('\\', "/"))
}

/// Read existing JSON config from path, or return empty object if file doesn't exist or is empty.
fn read_config(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    if content.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

/// Write JSON config to path, creating parent directories if needed.
fn write_config(path: &Path, config: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(path, content).map_err(|e| e.to_string())
}

/// Insert aikd as an MCP server entry into an object-style `mcpServers` field.
fn insert_mcp_server_object(config: &mut Value, binary: &str) {
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
}

// ─── Claude Code ───

fn claude_code_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".claude.json");
    if path.exists() || home.join(".claude").exists() {
        Some(path)
    } else {
        None
    }
}

fn write_claude_code(aikd_binary: &Path) -> Result<(), String> {
    let path = claude_code_path().ok_or("No home dir")?;
    let binary = norm_bin(aikd_binary)?;
    let mut config = read_config(&path)?;
    insert_mcp_server_object(&mut config, &binary);
    write_config(&path, &config)
}

fn claude_dir_mcp_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".claude").join("mcp.json");
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

fn write_claude_dir_mcp(aikd_binary: &Path) -> Result<(), String> {
    let path = claude_dir_mcp_path().ok_or("Claude mcp.json not found")?;
    let binary = norm_bin(aikd_binary)?;
    let mut config = read_config(&path)?;
    insert_mcp_server_object(&mut config, &binary);
    write_config(&path, &config)
}

// ─── Claude Desktop (GUI) ───

fn claude_desktop_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = std::env::var("APPDATA").ok().map(PathBuf::from)?;
    #[cfg(target_os = "macos")]
    let base = dirs::home_dir()?
        .join("Library")
        .join("Application Support");
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let base = dirs::home_dir()?.join(".config");

    let dir = base.join("Claude");
    let path = dir.join("claude_desktop_config.json");
    if path.exists() || dir.exists() {
        Some(path)
    } else {
        None
    }
}

fn write_claude_desktop(aikd_binary: &Path) -> Result<(), String> {
    let path = claude_desktop_path().ok_or("Claude Desktop not found")?;
    let binary = norm_bin(aikd_binary)?;
    let mut config = read_config(&path)?;
    insert_mcp_server_object(&mut config, &binary);
    write_config(&path, &config)
}

// ─── Antigravity (CLI & IDE) ───

fn antigravity_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let dir = home.join(".gemini").join("antigravity");
    let path = dir.join("mcp_config.json");
    if path.exists() || dir.exists() {
        Some(path)
    } else {
        None
    }
}

fn write_antigravity(aikd_binary: &Path) -> Result<(), String> {
    let path = antigravity_path().ok_or("Antigravity not found")?;
    let binary = norm_bin(aikd_binary)?;
    let mut config = read_config(&path)?;
    insert_mcp_server_object(&mut config, &binary);
    write_config(&path, &config)
}

fn antigravity_ide_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let dir = home.join(".gemini").join("antigravity-ide");
    let path = dir.join("mcp_config.json");
    if path.exists() || dir.exists() {
        Some(path)
    } else {
        None
    }
}

fn write_antigravity_ide(aikd_binary: &Path) -> Result<(), String> {
    let path = antigravity_ide_path().ok_or("Antigravity IDE not found")?;
    let binary = norm_bin(aikd_binary)?;
    let mut config = read_config(&path)?;
    insert_mcp_server_object(&mut config, &binary);
    write_config(&path, &config)
}

// ─── Gemini CLI ───

fn gemini_cli_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".gemini").join("settings.json");
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

fn write_gemini_cli(aikd_binary: &Path) -> Result<(), String> {
    let path = gemini_cli_path().ok_or("Gemini CLI not found")?;
    let binary = norm_bin(aikd_binary)?;
    let mut config = read_config(&path)?;
    insert_mcp_server_object(&mut config, &binary);
    write_config(&path, &config)
}

// ─── OpenAI Codex CLI (TOML) ───

fn codex_cli_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".codex").join("config.toml");
    if path.exists() || home.join(".codex").exists() {
        Some(path)
    } else {
        None
    }
}

fn write_codex_cli(aikd_binary: &Path) -> Result<(), String> {
    let path = codex_cli_path().ok_or("Codex CLI not found")?;
    let binary = norm_bin(aikd_binary)?;
    let mut content = if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| e.to_string())?
    } else {
        String::new()
    };
    if !content.contains("[mcp_servers.aikd]") {
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(&format!(
            "\n[mcp_servers.aikd]\ncommand = \"{binary}\"\nargs = [\"serve\"]\n"
        ));
        std::fs::write(&path, content).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ─── OpenCode CLI ───

fn opencode_cli_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".config").join("opencode").join("opencode.json");
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

fn write_opencode_cli(aikd_binary: &Path) -> Result<(), String> {
    let path = opencode_cli_path().ok_or("OpenCode CLI not found")?;
    let binary = norm_bin(aikd_binary)?;
    let mut config = read_config(&path)?;
    let mcp = config.get("mcp").cloned().unwrap_or(json!({}));
    let mut servers = mcp.as_object().cloned().unwrap_or_default();
    servers.insert(
        "aikd".into(),
        json!({
            "type": "local",
            "command": [binary, "serve"],
            "enabled": true,
        }),
    );
    config["mcp"] = json!(servers);
    write_config(&path, &config)
}

// ─── Pi CLI & OMP Agent CLI ───

fn pi_agent_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("PI_CODING_AGENT_DIR") {
        let p = PathBuf::from(dir).join("mcp.json");
        if p.exists()
            || Path::new(&std::env::var("PI_CODING_AGENT_DIR").unwrap_or_default()).exists()
        {
            return Some(p);
        }
    }
    let home = dirs::home_dir()?;
    let path = home.join(".pi").join("agent").join("mcp.json");
    if path.exists() || home.join(".pi").join("agent").exists() {
        Some(path)
    } else {
        None
    }
}

fn write_pi_agent(aikd_binary: &Path) -> Result<(), String> {
    let path = pi_agent_path().ok_or("Pi / OMP Agent CLI not found")?;
    let binary = norm_bin(aikd_binary)?;
    let mut config = read_config(&path)?;
    insert_mcp_server_object(&mut config, &binary);
    write_config(&path, &config)
}

// ─── Qwen Code CLI ───

fn qwen_cli_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".qwen").join("settings.json");
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

fn write_qwen_cli(aikd_binary: &Path) -> Result<(), String> {
    let path = qwen_cli_path().ok_or("Qwen Code CLI not found")?;
    let binary = norm_bin(aikd_binary)?;
    let mut config = read_config(&path)?;
    insert_mcp_server_object(&mut config, &binary);
    write_config(&path, &config)
}

// ─── Hermes Agent CLI (YAML) ───

fn hermes_cli_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".hermes").join("config.yaml");
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

fn write_hermes_cli(aikd_binary: &Path) -> Result<(), String> {
    let path = hermes_cli_path().ok_or("Hermes CLI not found")?;
    let binary = norm_bin(aikd_binary)?;
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut doc: serde_yaml::Value = serde_yaml::from_str(&raw).map_err(|e| e.to_string())?;
    let root = doc.as_mapping_mut().ok_or("Invalid YAML root")?;
    let mcp_key = serde_yaml::Value::String("mcp_servers".into());
    if !root.contains_key(&mcp_key) {
        root.insert(
            mcp_key.clone(),
            serde_yaml::Value::Mapping(serde_yaml::Mapping::new()),
        );
    }
    if let Some(mcp_map) = root.get_mut(&mcp_key).and_then(|v| v.as_mapping_mut()) {
        let mut entry = serde_yaml::Mapping::new();
        entry.insert(
            serde_yaml::Value::String("command".into()),
            serde_yaml::Value::String(binary),
        );
        entry.insert(
            serde_yaml::Value::String("args".into()),
            serde_yaml::Value::Sequence(vec![serde_yaml::Value::String("serve".into())]),
        );
        mcp_map.insert(
            serde_yaml::Value::String("aikd".into()),
            serde_yaml::Value::Mapping(entry),
        );
    }
    let out = serde_yaml::to_string(&doc).map_err(|e| e.to_string())?;
    std::fs::write(&path, out).map_err(|e| e.to_string())
}

// ─── Qoder ───

fn qoder_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".qoder").join("mcp.json");
    if path.exists() || home.join(".qoder").exists() {
        Some(path)
    } else {
        None
    }
}

fn write_qoder(aikd_binary: &Path) -> Result<(), String> {
    let path = qoder_path().ok_or("Qoder not found")?;
    let binary = norm_bin(aikd_binary)?;
    let mut config = read_config(&path)?;
    insert_mcp_server_object(&mut config, &binary);
    write_config(&path, &config)
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
    let binary = norm_bin(aikd_binary)?;
    let mut config = read_config(&path)?;
    insert_mcp_server_object(&mut config, &binary);
    write_config(&path, &config)
}

// ─── Cline ───

fn cline_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".cline").join("mcp.json");
    if path.exists() || home.join(".cline").exists() {
        return Some(path);
    }
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
    let binary = norm_bin(aikd_binary)?;

    if path
        .file_name()
        .map(|f| f == "settings.json")
        .unwrap_or(false)
    {
        let mut config = read_config(&path)?;
        config["cline.mcpServers"] = json!({
            "aikd": {
                "command": binary,
                "args": ["serve"],
            }
        });
        write_config(&path, &config)
    } else {
        let mut config = read_config(&path)?;
        insert_mcp_server_object(&mut config, &binary);
        write_config(&path, &config)
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
    let binary = norm_bin(aikd_binary)?;
    let mut config = read_config(&path)?;

    let mcp_servers = config.get("mcpServers").cloned().unwrap_or(json!([]));
    let mut servers = mcp_servers.as_array().cloned().unwrap_or_default();

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
        write_config(&path, &config)?;
    }

    Ok(())
}

// ─── Windsurf ───

fn windsurf_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".windsurf").join("mcp.json");
    if path.exists() || home.join(".windsurf").exists() {
        return Some(path);
    }
    let codeium_path = home
        .join(".codeium")
        .join("windsurf")
        .join("mcp_config.json");
    if codeium_path.exists() {
        return Some(codeium_path);
    }
    None
}

fn write_windsurf(aikd_binary: &Path) -> Result<(), String> {
    let path = windsurf_path().ok_or("Windsurf not found")?;
    let binary = norm_bin(aikd_binary)?;
    let mut config = read_config(&path)?;
    insert_mcp_server_object(&mut config, &binary);
    write_config(&path, &config)
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
    let binary = norm_bin(aikd_binary)?;
    let mut config = read_config(&path)?;
    insert_mcp_server_object(&mut config, &binary);
    write_config(&path, &config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_agents_returns_list() {
        let agents = all_agents();
        assert!(agents.len() >= 15);
    }

    #[test]
    fn test_agent_names() {
        let agents = all_agents();
        let names: Vec<&str> = agents.iter().map(|a| a.name).collect();
        assert!(names.contains(&"Claude Code"));
        assert!(names.contains(&"Cursor"));
        assert!(names.contains(&"Antigravity"));
        assert!(names.contains(&"Gemini CLI"));
        assert!(names.contains(&"OpenAI Codex CLI"));
        assert!(names.contains(&"OpenCode CLI"));
        assert!(names.contains(&"Pi / OMP Agent CLI"));
    }
}
