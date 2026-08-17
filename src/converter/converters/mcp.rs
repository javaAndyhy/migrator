//! MCP 转换器 — Codex config.toml ([mcp_servers]) → Qoder .mcp.json
//!
//! 来源: Codex 的 TOML mcp_servers 段格式（含嵌套变体）:
//!   [mcp_servers.<name>]
//!   command = "..." / args = [...]          # stdio server
//!   url = "..." / bearer_token_env_var = "..."  # HTTP server
//!   [mcp_servers.<name>.env]                # 嵌套 env → 合并为 server 的 env 字段
//!   [mcp_servers.<name>.tools.<t>]          # 嵌套 tools 段 → 忽略（Codex 特有）
//!
//! 目标: Qoder 兼容 Claude 的 JSON 格式:
//!   { "mcpServers": { "<name>": { "command", "args", "env", "url" } } }

use serde_json::{json, Value};

/// 将 Codex config.toml 内容转换为 .mcp.json 格式
pub fn convert_codex_mcp(config_toml: &str) -> (String, bool, Vec<String>) {
    let mut manual_notes = Vec::new();
    let mut servers: Vec<(String, Value)> = Vec::new();

    let lines: Vec<&str> = config_toml.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        if let Some(name) = parse_mcp_section_header(line) {
            // 跳过嵌套段（.env / .tools.*）— 由父段处理
            if is_nested_section(&name) {
                i += 1;
                continue;
            }
            let mut server = json!({});
            let mut env_vars: Vec<String> = Vec::new();
            let mut section_end = i + 1;
            while section_end < lines.len() {
                let l = lines[section_end].trim();
                if parse_mcp_section_header(l).is_some() {
                    break; // 下一个 mcp_servers 段
                }
                if let Some(v) = parse_key_value(l, "command") {
                    server["command"] = json!(v);
                } else if let Some(v) = parse_array(l, "args") {
                    server["args"] = json!(v);
                } else if let Some(v) = parse_array(l, "env_vars") {
                    env_vars = v;
                } else if let Some(v) = parse_key_value(l, "cwd") {
                    server["cwd"] = json!(v);
                } else if let Some(v) = parse_key_value(l, "url") {
                    server["url"] = json!(v);
                } else if let Some(v) = parse_key_value(l, "bearer_token_env_var") {
                    // Codex 特有 → 标准 MCP headers（Claude/Qoder 兼容）
                    server["headers"] = json!({
                        "Authorization": format!("Bearer {{env:{v}}}")
                    });
                    server["bearer_token_env_var"] = json!(v);
                    manual_notes.push(format!(
                        "mcp server {name}: bearer_token_env_var -> Authorization header (review)"
                    ));
                } else if l.starts_with("enabled") && !l.contains("true") {
                    manual_notes.push(format!("mcp server {name}: disabled, skipped"));
                }
                section_end += 1;
            }

            // 检查嵌套 env 段: [mcp_servers.<name>.env] KEY = value
            let nested_env = collect_nested_env(config_toml, &name);
            if !nested_env.is_empty() {
                server["env"] = json!(nested_env);
                manual_notes.push(format!(
                    "mcp server {name}: nested env merged (Codex-specific, review)"
                ));
            }

            // Codex env_vars → 标准 MCP env 对象（{env:VAR} 引用，Claude/Qoder 兼容）
            if !env_vars.is_empty() {
                let mut env_map = serde_json::Map::new();
                for var in &env_vars {
                    env_map.insert(
                        var.clone(),
                        json!(format!("{{env:{var}}}")),
                    );
                }
                // 若已有嵌套 env 合并进来，则追加
                if let Some(existing) = server["env"].as_object_mut() {
                    for (k, v) in env_map {
                        existing.entry(k).or_insert(v);
                    }
                } else {
                    server["env"] = json!(env_map);
                }
                manual_notes.push(format!(
                    "mcp server {name}: env_vars {:?} -> env object ({{env:VAR}} refs, review)",
                    env_vars
                ));
            }
            // Codex 特有字段不再原样输出（避免目标平台 schema 校验失败）
            if server.get("bearer_token_env_var").is_some() {
                server.as_object_mut().unwrap().remove("bearer_token_env_var");
            }
            if server.get("tools").is_some() {
                manual_notes.push(format!("mcp server {name}: nested tools config dropped"));
            }

            servers.push((name, server));
            i = section_end - 1;
        }
        i += 1;
    }

    if servers.is_empty() {
        return (
            "{\n  \"mcpServers\": {}\n}".to_string(),
            false,
            vec![],
        );
    }

    let mut out = String::from("{\n  \"mcpServers\": {\n");
    for (idx, (name, server)) in servers.iter().enumerate() {
        let comma = if idx == servers.len() - 1 { "" } else { "," };
        out.push_str(&format!("    \"{name}\": {}{}\n", server, comma));
    }
    out.push_str("  }\n}");

    let needs_review = !manual_notes.is_empty();
    (out, needs_review, manual_notes)
}

/// 解析 mcp_servers 段头: [mcp_servers.<name>] → Some(name)
/// 注意: 只匹配顶层 server（无嵌套点），嵌套段返回 None 由父处理
fn parse_mcp_section_header(line: &str) -> Option<String> {
    if !line.starts_with('[') || !line.ends_with(']') {
        return None;
    }
    let inner = &line[1..line.len() - 1];
    inner.strip_prefix("mcp_servers.").map(|n| n.to_string())
}

/// 嵌套段（含 . 的部分如 codegraph.env / codegraph.tools.x）
fn is_nested_section(name: &str) -> bool {
    name.contains('.')
}

/// 收集 [mcp_servers.<name>.env] 段的 key = value
fn collect_nested_env(config_toml: &str, name: &str) -> serde_json::Map<String, Value> {
    let mut result = serde_json::Map::new();
    let env_header = format!("[mcp_servers.{name}.env]");
    let mut in_env = false;
    for line in config_toml.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            in_env = t == env_header;
            continue;
        }
        if in_env {
            if let Some(eq) = t.find('=') {
                let k = t[..eq].trim().to_string();
                let v = t[eq + 1..].trim().trim_matches('"').trim_matches('\'').to_string();
                if !k.is_empty() {
                    result.insert(k, json!(v));
                }
            }
        }
    }
    result
}

/// 解析 key = "value"（支持单引号和双引号）
fn parse_key_value(line: &str, key: &str) -> Option<String> {
    let prefix = format!("{key} =");
    let t = line.trim_start();
    let v = t.strip_prefix(&prefix)?;
    let v = v.trim();
    let v = v.trim_matches('"').trim_matches('\'');
    Some(v.to_string())
}

/// 解析 key = ["a", "b"] 数组 → Vec<String>
fn parse_array(line: &str, key: &str) -> Option<Vec<String>> {
    let prefix = format!("{key} =");
    let t = line.trim_start();
    let v = t.strip_prefix(&prefix)?;
    let v = v.trim();
    if !v.starts_with('[') || !v.ends_with(']') {
        return None;
    }
    let inner = &v[1..v.len() - 1];
    let items = inner
        .split(',')
        .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|s| !s.is_empty())
        .collect();
    Some(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_simple_servers() {
        let toml = r#"
[mcp_servers.github]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
"#;
        let (out, _, _) = convert_codex_mcp(toml);
        assert!(out.contains("\"github\""));
        assert!(out.contains("\"npx\""));
    }

    #[test]
    fn nested_env_merged_into_server() {
        let toml = r#"
[mcp_servers.codegraph]
command = "codegraph"
args = ["serve"]

[mcp_servers.codegraph.env]
CODEGRAPH_MCP_TOOLS = "explore"

[mcp_servers.codegraph.tools.codegraph_explore]
approval_mode = "approve"
"#;
        let (out, needs_review, _) = convert_codex_mcp(toml);
        assert!(out.contains("\"codegraph\""));
        // 嵌套 env 合并为 server 的 env 字段
        assert!(out.contains("CODEGRAPH_MCP_TOOLS"));
        assert!(out.contains("\"explore\""));
        // 嵌套 tools 段不应成为独立 server
        assert!(!out.contains("codegraph.tools"));
        assert!(needs_review);
    }

    #[test]
    fn http_server_with_bearer_token() {
        let toml = r#"
[mcp_servers.github]
url = "https://api.githubcopilot.com/mcp/"
bearer_token_env_var = "GITHUB_PERSONAL_ACCESS_TOKEN"
"#;
        let (out, needs_review, _) = convert_codex_mcp(toml);
        assert!(out.contains("\"url\""));
        assert!(out.contains("api.githubcopilot.com"));
        // bearer_token_env_var → Authorization header
        assert!(out.contains("\"headers\""));
        assert!(out.contains("Bearer {env:GITHUB_PERSONAL_ACCESS_TOKEN}"));
        // Codex 特有字段不再原样输出
        assert!(!out.contains("\"bearer_token_env_var\""));
        assert!(needs_review);
    }

    #[test]
    fn no_mcp_servers_returns_empty() {
        let (out, _, _) = convert_codex_mcp("model = \"x\"\n[features]\nhooks = true\n");
        assert!(out.contains("\"mcpServers\": {}"));
    }

    #[test]
    fn env_vars_flagged_for_review() {
        let toml = r#"
[mcp_servers.db]
command = "start.ps1"
env_vars = ["DB_URL"]
"#;
        let (out, needs_review, notes) = convert_codex_mcp(toml);
        assert!(needs_review);
        assert!(notes.iter().any(|n| n.contains("env_vars")));
        // env_vars 转为标准 env 对象（{env:VAR} 引用）
        assert!(out.contains("\"env\""));
        assert!(out.contains("{env:DB_URL}"));
        // Codex 特有字段不再原样输出
        assert!(!out.contains("\"env_vars\""));
    }

    #[test]
    fn parses_key_value_and_array() {
        assert_eq!(
            parse_key_value("command = \"npx\"", "command"),
            Some("npx".to_string())
        );
        assert_eq!(
            parse_array("args = [\"-y\", \"pkg\"]", "args"),
            Some(vec!["-y".to_string(), "pkg".to_string()])
        );
    }
}
