//! MCP 转换器 — Codex config.toml ([mcp_servers]) ↔ JSON mcpServers
//!
//! v0.3 起改用真正的 TOML 解析（`toml` crate）与格式保留编辑（`toml_edit`），
//! 替换原先的手写逐行字符串解析。手写解析的已知缺陷:
//!   - 行尾注释（`command = "npx" # 备注`）会被并入取值
//!   - 多行数组（`args = [\n "-y",\n "pkg"\n]`）解析失败
//!   - 嵌套段（`[mcp_servers.x.env]`）之后属于同一 server 的字段被提前截断
//!   - env 值为非字符串（bool/整数）时被静默转成空串
//!
//! Codex 侧格式:
//!   [mcp_servers.<name>]
//!   command = "..." / args = [...]              # stdio server
//!   url = "..." / bearer_token_env_var = "..."  # HTTP server
//!   [mcp_servers.<name>.env]                    # 嵌套 env → 合并为 server 的 env 字段
//!   [mcp_servers.<name>.tools.<t>]              # 嵌套 tools 段 → 丢弃（Codex 特有）
//!
//! 目标侧（Claude 兼容 JSON，Qoder/Trae/灵码/WorkBuddy 同构）:
//!   { "mcpServers": { "<name>": { "command", "args", "env", "url", "headers" } } }

use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// Codex config.toml 顶层（只关心 mcp_servers，其余段忽略）
#[derive(Debug, Deserialize, Default)]
struct CodexConfig {
    #[serde(default)]
    mcp_servers: BTreeMap<String, CodexMcpServer>,
}

/// Codex 单个 MCP server 定义
#[derive(Debug, Deserialize, Default)]
struct CodexMcpServer {
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    /// 嵌套 [mcp_servers.<name>.env] — 值可能非字符串，用 toml::Value 兜住
    #[serde(default)]
    env: BTreeMap<String, toml::Value>,
    /// Codex 特有: 仅声明需透传的环境变量名
    #[serde(default)]
    env_vars: Vec<String>,
    cwd: Option<String>,
    url: Option<String>,
    /// Codex 特有: HTTP server 的 bearer token 环境变量名
    bearer_token_env_var: Option<String>,
    enabled: Option<bool>,
    /// Codex 特有: 嵌套 tools 审批配置 — 目标平台无对应物，丢弃并标记
    #[serde(default)]
    tools: Option<toml::Value>,
}

/// 将 Codex config.toml 内容转换为 mcpServers JSON 格式
///
/// 返回 (json_content, needs_review, manual_notes)。
/// TOML 解析失败时返回空 mcpServers 并标记需人工审查（绝不静默产出半成品）。
pub fn convert_codex_mcp(config_toml: &str) -> (String, bool, Vec<String>) {
    let mut manual_notes = Vec::new();

    let config: CodexConfig = match toml::from_str(config_toml) {
        Ok(c) => c,
        Err(e) => {
            return (
                empty_mcp_json(),
                true,
                vec![format!(
                    "Codex config.toml 解析失败，未迁移任何 MCP server: {e}"
                )],
            );
        }
    };

    if config.mcp_servers.is_empty() {
        return (empty_mcp_json(), false, vec![]);
    }

    let mut servers = Map::new();
    for (name, src) in &config.mcp_servers {
        let mut server = Map::new();

        if let Some(command) = &src.command {
            server.insert("command".into(), json!(command));
        }
        if !src.args.is_empty() {
            server.insert("args".into(), json!(src.args));
        }
        if let Some(cwd) = &src.cwd {
            server.insert("cwd".into(), json!(cwd));
        }
        if let Some(url) = &src.url {
            server.insert("url".into(), json!(url));
        }

        // 嵌套 env 段 → 标准 env 对象（非字符串值转为其 TOML 字面量文本）
        let mut env_map = Map::new();
        for (k, v) in &src.env {
            env_map.insert(k.clone(), json!(toml_value_to_string(v)));
        }
        if !env_map.is_empty() {
            manual_notes.push(format!(
                "mcp server {name}: nested env merged (Codex-specific, review)"
            ));
        }

        // Codex env_vars → 标准 env 对象的 {env:VAR} 引用（已有同名 key 不覆盖）
        if !src.env_vars.is_empty() {
            for var in &src.env_vars {
                env_map
                    .entry(var.clone())
                    .or_insert_with(|| json!(format!("{{env:{var}}}")));
            }
            manual_notes.push(format!(
                "mcp server {name}: env_vars {:?} -> env object ({{env:VAR}} refs, review)",
                src.env_vars
            ));
        }
        if !env_map.is_empty() {
            server.insert("env".into(), Value::Object(env_map));
        }

        // bearer_token_env_var → 标准 MCP Authorization header
        if let Some(var) = &src.bearer_token_env_var {
            server.insert(
                "headers".into(),
                json!({ "Authorization": format!("Bearer {{env:{var}}}") }),
            );
            manual_notes.push(format!(
                "mcp server {name}: bearer_token_env_var -> Authorization header (review)"
            ));
        }

        if src.tools.is_some() {
            manual_notes.push(format!("mcp server {name}: nested tools config dropped"));
        }
        if src.enabled == Some(false) {
            manual_notes.push(format!(
                "mcp server {name}: disabled in Codex, migrated but review before enabling"
            ));
        }

        servers.insert(name.clone(), Value::Object(server));
    }

    let out = serde_json::to_string_pretty(&json!({ "mcpServers": servers }))
        .unwrap_or_else(|_| empty_mcp_json());
    let needs_review = !manual_notes.is_empty();
    (out, needs_review, manual_notes)
}

/// 将 mcpServers JSON 转换为 Codex config.toml 的 [mcp_servers] 段
///
/// 反向迁移：国产平台 → Codex。用 toml_edit 生成，保证转义与类型正确。
///   command/args/url/cwd/env → 对应 TOML 键
///   headers.Authorization("Bearer {env:X}") → bearer_token_env_var = "X"（近似，审查）
///   type/_setup（sse/http/streamable-http 特有）→ 丢弃并标记审查
pub fn convert_mcp_json_to_toml(json_content: &str) -> (String, bool, Vec<String>) {
    use toml_edit::{Array, DocumentMut, Item, Table, value};

    let mut manual_notes = Vec::new();
    let v: Value = match serde_json::from_str(json_content) {
        Ok(v) => v,
        Err(e) => {
            return (
                String::new(),
                true,
                vec![format!("MCP JSON 解析失败，请检查源文件: {e}")],
            );
        }
    };
    let Some(servers) = v.get("mcpServers").and_then(|s| s.as_object()) else {
        return (String::new(), false, vec![]);
    };

    let mut doc = DocumentMut::new();
    let mut root = Table::new();
    root.set_implicit(true); // 不输出裸 [mcp_servers] 段头

    for (name, server) in servers {
        let mut tbl = Table::new();
        if let Some(command) = server.get("command").and_then(|c| c.as_str()) {
            tbl.insert("command", value(command));
        }
        if let Some(args) = server.get("args").and_then(|a| a.as_array()) {
            let mut arr = Array::new();
            for a in args {
                if let Some(s) = a.as_str() {
                    arr.push(s);
                }
            }
            tbl.insert("args", value(arr));
        }
        if let Some(url) = server.get("url").and_then(|u| u.as_str()) {
            tbl.insert("url", value(url));
        }
        if let Some(cwd) = server.get("cwd").and_then(|c| c.as_str()) {
            tbl.insert("cwd", value(cwd));
        }
        if let Some(headers) = server.get("headers").and_then(|h| h.as_object())
            && let Some(auth) = headers.get("Authorization").and_then(|a| a.as_str())
            && let Some(var) = extract_env_var(auth)
        {
            tbl.insert("bearer_token_env_var", value(var.as_str()));
            manual_notes.push(format!(
                "mcp server {name}: Authorization header -> bearer_token_env_var (approx)"
            ));
        }
        // env 作为嵌套子表输出（[mcp_servers.<name>.env]）
        if let Some(env) = server.get("env").and_then(|e| e.as_object()) {
            let mut env_tbl = Table::new();
            for (k, val) in env {
                let sv = match val {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                env_tbl.insert(k, value(sv.as_str()));
            }
            tbl.insert("env", Item::Table(env_tbl));
        }
        if server.get("type").is_some() || server.get("_setup").is_some() {
            manual_notes.push(format!(
                "mcp server {name}: type/_setup dropped (Codex HTTP servers use url only)"
            ));
        }
        root.insert(name, Item::Table(tbl));
    }

    doc.insert("mcp_servers", Item::Table(root));
    let needs_review = !manual_notes.is_empty();
    (doc.to_string(), needs_review, manual_notes)
}

/// 段级合并：把 incoming 的 [mcp_servers.*] 合并进 existing config.toml
///
/// 契约（P0 安全边界 — 保护用户真实 Codex 配置）:
///   - existing 的所有其他段（model / provider / API key / 注释 / 空行 / 键序）逐字保留
///   - 同名 server 以 incoming 为准；existing 独有的 server 保留
///   - existing 解析失败 → 返回 Err，拒绝写入（绝不用整文件覆盖"修复"用户文件）
///   - existing 的 mcp_servers 是 inline table 时 → 返回 Err，要求人工处理
pub fn merge_mcp_servers_toml(existing: &str, incoming: &str) -> Result<String, String> {
    use toml_edit::{DocumentMut, Item, Table};

    let mut doc: DocumentMut = existing
        .parse()
        .map_err(|e| format!("目标 config.toml 解析失败，拒绝写入以免损坏用户配置: {e}"))?;
    let inc: DocumentMut = incoming
        .parse()
        .map_err(|e| format!("待写入的 [mcp_servers] 段解析失败: {e}"))?;

    let Some(inc_tbl) = inc.get("mcp_servers").and_then(|i| i.as_table()) else {
        // 源侧无 server → 目标原样保留
        return Ok(existing.to_string());
    };

    // 写入位置基线：取目标文档已有段的最大 position，新段一律排在其后。
    // 必须在插入前计算，否则会被 incoming 携带的旧 position 污染。
    let mut next_pos = max_table_position(doc.as_table()) + 1;

    if doc.get("mcp_servers").is_none() {
        let mut placeholder = Table::new();
        placeholder.set_implicit(true);
        doc.insert("mcp_servers", Item::Table(placeholder));
    }
    let dst = doc
        .get_mut("mcp_servers")
        .and_then(|i| i.as_table_mut())
        .ok_or_else(|| {
            "目标 config.toml 的 mcp_servers 不是标准段（可能是 inline table），\
             拒绝自动写入，请手动合并"
                .to_string()
        })?;

    for (name, item) in inc_tbl.iter() {
        // 关键: clone 出来的段带着 incoming 文档里的旧 position（通常是 0/1 这类小值），
        // 直接插入会让新段被渲染到目标已有段之前 —— 表现为
        // [mcp_servers.x] 夹在 [model_providers] 与 [history] 之间、
        // 且 [mcp_servers.x.env] 与父段被 [history] 隔开。
        // 语义虽合法（TOML 表头是绝对路径），但人工编辑时极易误改到相邻段，
        // 因此这里对整个子树重新编号，保证「追加到末尾 + 父子相邻」。
        let mut cloned = item.clone();
        reposition_subtree(&mut cloned, &mut next_pos);
        dst.insert(name, cloned);
    }

    Ok(doc.to_string())
}

/// 递归求表内已有段的最大 position（含数组表）
fn max_table_position(tbl: &toml_edit::Table) -> usize {
    use toml_edit::Item;
    let mut max = tbl.position().unwrap_or(0);
    for (_, item) in tbl.iter() {
        match item {
            Item::Table(t) => max = max.max(max_table_position(t)),
            Item::ArrayOfTables(arr) => {
                for t in arr.iter() {
                    max = max.max(max_table_position(t));
                }
            }
            _ => {}
        }
    }
    max
}

/// 对待插入子树重新编号 position（父段先于子段，保证渲染时相邻）
///
/// 同时为无有效前缀的段补一个空行，避免新段紧贴上一段段尾。
fn reposition_subtree(item: &mut toml_edit::Item, next: &mut usize) {
    use toml_edit::Item;
    match item {
        Item::Table(t) => {
            // 隐式段不渲染段头，无需占位
            if !t.is_implicit() {
                t.set_position(*next);
                *next += 1;
                let prefix_blank = t
                    .decor()
                    .prefix()
                    .and_then(|p| p.as_str())
                    .map(|s| s.trim().is_empty())
                    .unwrap_or(true);
                if prefix_blank {
                    t.decor_mut().set_prefix("\n");
                }
            }
            for (_, child) in t.iter_mut() {
                reposition_subtree(child, next);
            }
        }
        Item::ArrayOfTables(arr) => {
            for t in arr.iter_mut() {
                t.set_position(*next);
                *next += 1;
            }
        }
        _ => {}
    }
}

/// 空 mcpServers JSON（保持与 serde_json pretty 输出一致）
fn empty_mcp_json() -> String {
    serde_json::to_string_pretty(&json!({ "mcpServers": {} }))
        .unwrap_or_else(|_| "{\n  \"mcpServers\": {}\n}".to_string())
}

/// TOML 值 → 字符串（字符串取原值，其余取字面量文本，避免静默丢数据）
fn toml_value_to_string(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// 从 "Bearer {env:VAR}" 提取 VAR
fn extract_env_var(auth: &str) -> Option<String> {
    let start = auth.find("{env:")? + 5;
    let rel = &auth[start..];
    let end = rel.find('}')?;
    Some(rel[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_simple_servers() {
        let toml_src = r#"
[mcp_servers.github]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
"#;
        let (out, _, _) = convert_codex_mcp(toml_src);
        assert!(out.contains("\"github\""));
        assert!(out.contains("\"npx\""));
    }

    #[test]
    fn nested_env_merged_into_server() {
        let toml_src = r#"
[mcp_servers.codegraph]
command = "codegraph"
args = ["serve"]

[mcp_servers.codegraph.env]
CODEGRAPH_MCP_TOOLS = "explore"

[mcp_servers.codegraph.tools.codegraph_explore]
approval_mode = "approve"
"#;
        let (out, needs_review, _) = convert_codex_mcp(toml_src);
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
        let toml_src = r#"
[mcp_servers.github]
url = "https://api.githubcopilot.com/mcp/"
bearer_token_env_var = "GITHUB_PERSONAL_ACCESS_TOKEN"
"#;
        let (out, needs_review, _) = convert_codex_mcp(toml_src);
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
        let toml_src = r#"
[mcp_servers.db]
command = "start.ps1"
env_vars = ["DB_URL"]
"#;
        let (out, needs_review, notes) = convert_codex_mcp(toml_src);
        assert!(needs_review);
        assert!(notes.iter().any(|n| n.contains("env_vars")));
        // env_vars 转为标准 env 对象（{env:VAR} 引用）
        assert!(out.contains("\"env\""));
        assert!(out.contains("{env:DB_URL}"));
        // Codex 特有字段不再原样输出
        assert!(!out.contains("\"env_vars\""));
    }

    // ---- 回归: 手写逐行解析的已知缺陷 ----

    #[test]
    fn trailing_comments_are_not_part_of_values() {
        let toml_src = r#"
[mcp_servers.gh]
command = "npx" # 用 npx 启动
args = ["-y", "pkg"] # 参数
"#;
        let (out, _, _) = convert_codex_mcp(toml_src);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["mcpServers"]["gh"]["command"], "npx");
        assert_eq!(v["mcpServers"]["gh"]["args"][1], "pkg");
    }

    #[test]
    fn multiline_arrays_are_parsed() {
        let toml_src = r#"
[mcp_servers.gh]
command = "npx"
args = [
  "-y",
  "@scope/pkg",
]
"#;
        let (out, _, _) = convert_codex_mcp(toml_src);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["mcpServers"]["gh"]["args"][0], "-y");
        assert_eq!(v["mcpServers"]["gh"]["args"][1], "@scope/pkg");
    }

    #[test]
    fn fields_after_nested_section_are_not_lost() {
        // 旧实现在遇到 [mcp_servers.x.env] 后截断，url 会丢失
        let toml_src = r#"
[mcp_servers.svc]
command = "svc"

[mcp_servers.svc.env]
KEY = "v"
"#;
        let (out, _, _) = convert_codex_mcp(toml_src);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["mcpServers"]["svc"]["command"], "svc");
        assert_eq!(v["mcpServers"]["svc"]["env"]["KEY"], "v");
    }

    #[test]
    fn non_string_env_values_are_preserved() {
        let toml_src = r#"
[mcp_servers.svc]
command = "svc"

[mcp_servers.svc.env]
DEBUG = true
PORT = 8080
"#;
        let (out, _, _) = convert_codex_mcp(toml_src);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["mcpServers"]["svc"]["env"]["DEBUG"], "true");
        assert_eq!(v["mcpServers"]["svc"]["env"]["PORT"], "8080");
    }

    #[test]
    fn malformed_toml_is_reported_not_silently_emptied() {
        let (out, needs_review, notes) = convert_codex_mcp("[mcp_servers.broken\ncommand = ");
        assert!(needs_review, "解析失败必须标记需人工审查");
        assert!(notes.iter().any(|n| n.contains("解析失败")));
        assert!(out.contains("\"mcpServers\": {}"));
    }

    #[test]
    fn json_to_toml_reverse() {
        let json_src = r#"{"mcpServers":{
            "github":{"url":"https://api.githubcopilot.com/mcp/","headers":{"Authorization":"Bearer {env:GITHUB_TOKEN}"}},
            "local":{"command":"npx","args":["-y","demo"],"env":{"KEY":"val"}}
        }}"#;
        let (out, needs_review, notes) = convert_mcp_json_to_toml(json_src);
        assert!(out.contains("[mcp_servers.github]"));
        assert!(out.contains("url = \"https://api.githubcopilot.com/mcp/\""));
        assert!(out.contains("bearer_token_env_var = \"GITHUB_TOKEN\""));
        assert!(out.contains("[mcp_servers.local]"));
        assert!(out.contains("command = \"npx\""));
        assert!(out.contains("[mcp_servers.local.env]"));
        assert!(out.contains("KEY = \"val\""));
        assert!(needs_review);
        assert!(notes.iter().any(|n| n.contains("bearer_token_env_var")));
        // 生成物必须是合法 TOML（旧手写生成无此保证）
        let parsed: toml::Value = toml::from_str(&out).expect("生成的 TOML 必须可解析");
        assert_eq!(
            parsed["mcp_servers"]["local"]["args"][1].as_str(),
            Some("demo")
        );
    }

    #[test]
    fn json_to_toml_escapes_special_characters() {
        let json_src = r#"{"mcpServers":{"win":{"command":"C:\\tools\\srv.exe","args":["--msg","say \"hi\""]}}}"#;
        let (out, _, _) = convert_mcp_json_to_toml(json_src);
        let parsed: toml::Value = toml::from_str(&out).expect("含反斜杠/引号仍须合法");
        assert_eq!(
            parsed["mcp_servers"]["win"]["command"].as_str(),
            Some("C:\\tools\\srv.exe")
        );
        assert_eq!(
            parsed["mcp_servers"]["win"]["args"][1].as_str(),
            Some("say \"hi\"")
        );
    }

    #[test]
    fn json_to_toml_handles_bad_json() {
        let (_, needs_review, notes) = convert_mcp_json_to_toml("not json");
        assert!(needs_review);
        assert!(notes.iter().any(|n| n.contains("解析失败")));
    }

    #[test]
    fn extracts_env_var_from_auth_header() {
        assert_eq!(
            extract_env_var("Bearer {env:GITHUB_PERSONAL_ACCESS_TOKEN}"),
            Some("GITHUB_PERSONAL_ACCESS_TOKEN".to_string())
        );
        assert_eq!(extract_env_var("Bearer static"), None);
    }

    // ---- P0: config.toml 段级合并（保护用户真实配置） ----

    #[test]
    fn toml_merge_preserves_other_sections_and_comments() {
        let existing = r#"# 我的 Codex 配置
model = "gpt-5-codex"
model_provider = "openai"

[model_providers.openai]
name = "OpenAI"
env_key = "OPENAI_API_KEY"   # 千万别丢

[mcp_servers.old-one]
command = "old"
"#;
        let incoming = r#"[mcp_servers.new-one]
command = "new"
"#;
        let merged = merge_mcp_servers_toml(existing, incoming).unwrap();
        // 其余段与注释逐字保留
        assert!(merged.contains("# 我的 Codex 配置"));
        assert!(merged.contains("model = \"gpt-5-codex\""));
        assert!(merged.contains("model_provider = \"openai\""));
        assert!(merged.contains("[model_providers.openai]"));
        assert!(merged.contains("env_key = \"OPENAI_API_KEY\""));
        assert!(merged.contains("# 千万别丢"));
        // 已有 server 保留 + 新 server 加入
        assert!(merged.contains("[mcp_servers.old-one]"));
        assert!(merged.contains("[mcp_servers.new-one]"));
        // 合并结果必须仍是合法 TOML
        let parsed: toml::Value = toml::from_str(&merged).unwrap();
        assert_eq!(parsed["model"].as_str(), Some("gpt-5-codex"));
        assert_eq!(
            parsed["mcp_servers"]["old-one"]["command"].as_str(),
            Some("old")
        );
        assert_eq!(
            parsed["mcp_servers"]["new-one"]["command"].as_str(),
            Some("new")
        );
    }

    #[test]
    fn toml_merge_overrides_same_named_server() {
        let existing = "[mcp_servers.dup]\ncommand = \"old\"\n";
        let incoming = "[mcp_servers.dup]\ncommand = \"new\"\n";
        let merged = merge_mcp_servers_toml(existing, incoming).unwrap();
        let parsed: toml::Value = toml::from_str(&merged).unwrap();
        assert_eq!(
            parsed["mcp_servers"]["dup"]["command"].as_str(),
            Some("new")
        );
    }

    #[test]
    fn toml_merge_into_empty_target_creates_section() {
        let merged = merge_mcp_servers_toml("", "[mcp_servers.a]\ncommand = \"x\"\n").unwrap();
        let parsed: toml::Value = toml::from_str(&merged).unwrap();
        assert_eq!(parsed["mcp_servers"]["a"]["command"].as_str(), Some("x"));
    }

    #[test]
    fn toml_merge_rejects_unparsable_target() {
        // 目标文件损坏时必须拒绝写入，而不是覆盖掉
        let err =
            merge_mcp_servers_toml("[broken\n", "[mcp_servers.a]\ncommand = \"x\"\n").unwrap_err();
        assert!(err.contains("拒绝写入"), "错误信息需说明拒绝写入: {err}");
    }

    #[test]
    fn toml_merge_keeps_target_when_incoming_empty() {
        let existing = "model = \"m\"\n";
        let merged = merge_mcp_servers_toml(existing, "").unwrap();
        assert_eq!(merged, existing);
    }

    #[test]
    fn toml_merge_preserves_nested_env_of_incoming_server() {
        let existing = "model = \"m\"\n";
        let incoming = "[mcp_servers.svc]\ncommand = \"s\"\n\n[mcp_servers.svc.env]\nK = \"v\"\n";
        let merged = merge_mcp_servers_toml(existing, incoming).unwrap();
        let parsed: toml::Value = toml::from_str(&merged).unwrap();
        assert_eq!(parsed["mcp_servers"]["svc"]["env"]["K"].as_str(), Some("v"));
        assert_eq!(parsed["model"].as_str(), Some("m"));
    }

    /// 版式回归: 新段必须追加到文档末尾，不得插进已有段之间
    ///
    /// 早期实现里 toml_edit 把无 position 的新段排在最前，产出:
    ///   [model_providers.openai] ... [mcp_servers.new] ... [history] ... [mcp_servers.new.env]
    /// 语义虽合法（表头是绝对路径），但父子段被隔开、新段紧贴他人段尾，
    /// 用户手工编辑时极易误改到相邻段。
    #[test]
    fn toml_merge_appends_new_sections_after_existing_ones() {
        let existing = "\
model = \"m\"

[model_providers.openai]
env_key = \"OPENAI_API_KEY\"

[history]
persistence = \"save-all\"
";
        let incoming = "[mcp_servers.svc]\ncommand = \"s\"\n\n[mcp_servers.svc.env]\nK = \"v\"\n";
        let merged = merge_mcp_servers_toml(existing, incoming).unwrap();

        let pos = |needle: &str| {
            merged
                .find(needle)
                .unwrap_or_else(|| panic!("缺少段 {needle}\n---\n{merged}"))
        };
        // 已有段保持原有先后顺序
        assert!(pos("[model_providers.openai]") < pos("[history]"));
        // 新段排在全部已有段之后
        assert!(
            pos("[history]") < pos("[mcp_servers.svc]"),
            "新段必须追加在已有段之后:\n{merged}"
        );
        // 父段与子段相邻，中间不得夹入他人段
        let between = &merged[pos("[mcp_servers.svc]")..pos("[mcp_servers.svc.env]")];
        assert!(
            !between.contains("[history]") && !between.contains("[model_providers"),
            "父子段之间不得夹入其他段:\n{merged}"
        );
        // 新段前留空行，避免紧贴上一段
        assert!(
            merged.contains("\n\n[mcp_servers.svc]"),
            "新段前需空行:\n{merged}"
        );
        // 仍是合法 TOML 且值正确
        let parsed: toml::Value = toml::from_str(&merged).unwrap();
        assert_eq!(parsed["mcp_servers"]["svc"]["env"]["K"].as_str(), Some("v"));
        assert_eq!(parsed["history"]["persistence"].as_str(), Some("save-all"));
    }
}
