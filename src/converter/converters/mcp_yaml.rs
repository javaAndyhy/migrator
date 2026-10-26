//! dsh（DeepSeek Harness）MCP 转换器 — mcpServers JSON ↔ cordis patch YAML 插件条目
//!
//! ## 为什么需要独立转换器
//!
//! dsh 的 MCP **不是**一个 JSON 配置文件，而是 profile 的 cordis patch 里的**插件条目**：
//!
//! ```yaml
//! - id: mcp-github
//!   name: '@deepseek-ai/dsh-mcp-client'
//!   config:
//!     serverName: github
//!     transport: stdio
//!     command: npx
//!     args: ['-y', '@modelcontextprotocol/server-github']
//!     env:
//!       GITHUB_TOKEN: !!js process.env.GITHUB_TOKEN
//! ```
//!
//! 依据 `@deepseek-ai/dsh-mcp-client` README（本机 `~/.dsh/profiles/node_modules`）。
//!
//! ## 写入策略: 追加托管块，不做整文件重组
//!
//! 用户手写的 patch 里存在 `!!js` 表达式、注释、其他插件条目。**解析再序列化会摧毁它们**
//! （serde_yaml 会丢注释、把 `!!js` 变回普通 tagged 值、重排键序）。因此写入走
//! 「YAML 注释形式的托管块」：
//!
//! ```yaml
//! # <!-- BEGIN MIGRATOR:dsh:mcp -->
//! - id: mcp-github
//!   ...
//! # <!-- END MIGRATOR:dsh:mcp -->
//! ```
//!
//! 注释行对 YAML 解析器不可见，所以文件语义不变；重复迁移只重写块内内容，块外逐字保留。
//!
//! ## 反向读取
//!
//! 只解析**托管块内**的内容（本模块自己的输出子集，结构可控），产出标准
//! `{"mcpServers": {...}}` JSON，供 `mcp` 转换器继续转 TOML/其他 JSON。
//! 块外的用户条目视为「非本工具管理」，不读取（避免把用户的私有配置误当迁移源）。

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

/// 托管块起始标记（YAML 注释形式，对解析器不可见）
pub const DSH_MCP_BEGIN: &str = "# <!-- BEGIN MIGRATOR:dsh:mcp -->";
/// 托管块结束标记
pub const DSH_MCP_END: &str = "# <!-- END MIGRATOR:dsh:mcp -->";
/// dsh MCP 客户端插件全名
pub const DSH_MCP_PLUGIN: &str = "@deepseek-ai/dsh-mcp-client";

/// upsert 结果
#[derive(Debug, Clone)]
pub struct CordisUpsert {
    /// 合并后的完整文件内容
    pub content: String,
    /// 因文件块外已存在同名 id 而跳过的 serverName
    pub skipped: Vec<String>,
}

/// mcpServers JSON → cordis patch YAML 条目片段
///
/// 返回 `(fragment, needs_review, notes)`。fragment 不含托管块标记，由
/// [`upsert_cordis_mcp_block`] 负责包裹与合并。
pub fn convert_json_mcp_to_cordis(json_mcp: &str) -> (String, bool, Vec<String>) {
    let mut notes = Vec::new();

    let parsed: Value = match serde_json::from_str(json_mcp) {
        Ok(v) => v,
        Err(e) => {
            return (
                String::new(),
                true,
                vec![format!("源 MCP 配置解析失败，未迁移任何 server: {e}")],
            );
        }
    };

    let servers = parsed
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    if servers.is_empty() {
        return (String::new(), false, vec!["源 MCP 配置不含任何 server".into()]);
    }

    let mut chunks: Vec<String> = Vec::new();
    for (name, def) in &servers {
        // WorkBuddy 等客户端会写 disabled 标记；dsh 无禁用开关，跳过而不是假装禁用
        if def.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false) {
            notes.push(format!("server {name}: 源侧已 disabled，dsh 无禁用开关 → 已跳过"));
            continue;
        }
        match build_entry(name, def) {
            Ok(Some((chunk, mut entry_notes))) => {
                notes.append(&mut entry_notes);
                chunks.push(chunk);
            }
            Ok(None) => {
                notes.push(format!(
                    "server {name}: 既无 command 也无 url，无法判定 stdio/http，已跳过"
                ));
            }
            Err(e) => notes.push(format!("server {name}: 生成 cordis 条目失败: {e}")),
        }
    }

    let fragment = chunks.join("");
    let needs_review = !notes.is_empty();
    (fragment, needs_review, notes)
}

/// 生成单个 server 的 cordis 插件条目（2 空格缩进的 YAML 片段，以 `- id:` 开头）
fn build_entry(name: &str, def: &Value) -> Result<Option<(String, Vec<String>)>> {
    let mut notes = Vec::new();

    // serverName 契约: [A-Za-z0-9_-]{1,32}（dsh 用它对工具名做命名空间）
    let server = normalize_server_name(name);
    if server != name {
        notes.push(format!(
            "server {name}: serverName 归一化为 `{server}`（dsh 仅允许 [A-Za-z0-9_-]{{1,32}}）"
        ));
    }

    let url = def.get("url").and_then(|v| v.as_str());
    let command = def.get("command").and_then(|v| v.as_str());
    let transport = if url.is_some() {
        "streamable-http"
    } else if command.is_some() {
        "stdio"
    } else {
        return Ok(None);
    };

    let mut out = String::new();
    out.push_str(&format!("- id: mcp-{server}\n"));
    out.push_str(&format!("  name: '{DSH_MCP_PLUGIN}'\n"));
    out.push_str("  config:\n");
    out.push_str(&format!("    serverName: {}\n", yaml_quote(&server)));
    out.push_str(&format!("    transport: {transport}\n"));

    if transport == "stdio" {
        let cmd = command.expect("stdio transport requires command");
        out.push_str(&format!("    command: {}\n", yaml_quote(cmd)));

        if let Some(args) = def.get("args").and_then(|v| v.as_array()) {
            let items: Vec<String> = args
                .iter()
                .filter_map(|a| a.as_str())
                .map(yaml_quote)
                .collect();
            if !items.is_empty() {
                out.push_str("    args:\n");
                for it in items {
                    out.push_str(&format!("      - {it}\n"));
                }
            }
            if args.len() != args.iter().filter(|a| a.is_string()).count() {
                notes.push(format!("server {name}: args 含非字符串项，已忽略非字符串部分"));
            }
        }
        if let Some(cwd) = def.get("cwd").and_then(|v| v.as_str()) {
            out.push_str(&format!("    cwd: {}\n", yaml_quote(cwd)));
        }
        if let Some(env) = def.get("env").and_then(|v| v.as_object()) {
            if !env.is_empty() {
                out.push_str("    env:\n");
                for (k, v) in env {
                    match v.as_str() {
                        Some(s) => out.push_str(&format!("      {k}: {}\n", yaml_js_or_scalar(s))),
                        None => {
                            // 非字符串 env（数字/布尔）→ 转成字面量文本，避免 dsh 侧类型错误
                            out.push_str(&format!(
                                "      {k}: {}\n",
                                yaml_quote(&v.to_string())
                            ));
                            notes.push(format!("server {name}: env[{k}] 非字符串，已转文本"));
                        }
                    }
                }
            }
        }
    } else {
        let url = url.expect("http transport requires url");
        out.push_str(&format!("    url: {}\n", yaml_quote(url)));
        if let Some(headers) = def.get("headers").and_then(|v| v.as_object()) {
            if !headers.is_empty() {
                out.push_str("    headers:\n");
                for (k, v) in headers {
                    match v.as_str() {
                        Some(s) => out.push_str(&format!("      {k}: {}\n", yaml_js_or_scalar(s))),
                        None => out.push_str(&format!("      {k}: {}\n", yaml_quote(&v.to_string()))),
                    }
                }
            }
        }
    }

    Ok(Some((out, notes)))
}

/// serverName 归一化: 只保留 [A-Za-z0-9_-]，截断到 32 字符
fn normalize_server_name(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if s.is_empty() {
        s = "server".to_string();
    }
    if s.len() > 32 {
        s.truncate(32);
    }
    s
}

/// YAML 单引号标量（内部单引号翻倍转义，最安全的通用形式）
fn yaml_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// `{env:VAR}` 占位符 → dsh 的 `!!js <expr>`
///
/// 契约与 WorkBuddy / Claude / Codex 侧的 `{env:VAR}` 保持可逆：
/// 反向读取时 `process.env.VAR` 会被还原为 `{env:VAR}`。
///
/// 注意: JS 表达式一律用**单引号**包裹成带 tag 的标量。
/// 裸写 ``!!js `Bearer ${...}` `` 是非法 YAML —— 反引号是 YAML 保留指示符，
/// 不能出现在 plain scalar 的起始位置（serde_yaml / dsh 会直接解析失败）。
fn yaml_js_or_scalar(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("{env:").and_then(|r| r.strip_suffix('}')) {
        if is_ident(rest) {
            return yaml_js_expr(&format!("process.env.{rest}"));
        }
    }
    if s.contains("{env:") {
        // 内嵌占位符（如 "Bearer {env:TOKEN}"）→ JS 模板字符串
        let mut out = String::from("`");
        let mut rest = s;
        while let Some(idx) = rest.find("{env:") {
            let (head, tail) = rest.split_at(idx);
            out.push_str(&escape_template(head));
            let after = &tail["{env:".len()..];
            match after.find('}') {
                Some(end) => {
                    let var = &after[..end];
                    if is_ident(var) {
                        out.push_str(&format!("${{process.env.{var}}}"));
                    } else {
                        out.push_str(&escape_template(&format!("{{env:{var}}}")));
                    }
                    rest = &after[end + 1..];
                }
                None => {
                    out.push_str(&escape_template(tail));
                    rest = "";
                }
            }
        }
        out.push_str(&escape_template(rest));
        out.push('`');
        return yaml_js_expr(&out);
    }
    yaml_quote(s)
}

/// 带 `!!js` tag 的 YAML 标量（单引号包裹 JS 源码）
fn yaml_js_expr(source: &str) -> String {
    format!("!!js '{}'", source.replace('\'', "''"))
}

/// 环境变量名合法性（避免把 `{env:foo bar}` 之类写进 JS 表达式）
fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !s.starts_with(|c: char| c.is_ascii_digit())
}

/// JS 模板字符串里的转义（反引号 / 反斜杠 / ${）
fn escape_template(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('`', "\\`")
        .replace("${", "\\${")
}

/// 把 cordis patch 片段合并进既有文件（托管块 upsert）
///
/// 契约:
///   - 块外内容逐字保留（注释、`!!js`、其他插件条目都不动）
///   - 重复迁移重写同一块，不叠加第二块
///   - 块外已存在同名 `id: mcp-<name>` 时跳过该 server（避免 dsh 加载期重复 id 报错）
///   - 文件以 `[]` 空数组结尾时先移除 `[]`（否则追加序列项会产出非法 YAML）
pub fn upsert_cordis_mcp_block(existing: &str, fragment: &str) -> Result<CordisUpsert> {
    let fragment = fragment.trim_end_matches(['\n', '\r']);
    if fragment.trim().is_empty() {
        return Ok(CordisUpsert {
            content: existing.to_string(),
            skipped: Vec::new(),
        });
    }

    // 1. 切出块外内容（若已有托管块则替换，否则追加）
    let outside = match extract_cordis_mcp_block(existing) {
        Some(_) => remove_block(existing)?,
        None => existing.to_string(),
    };

    // 2. 块外已占用的 id → 跳过同名 server，避免重复 id
    let occupied = collect_ids(&outside);
    let mut kept: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for (id, chunk) in split_entries(fragment) {
        if occupied.contains(&id) {
            skipped.push(id.trim_start_matches("mcp-").to_string());
        } else {
            kept.push(chunk);
        }
    }

    if kept.is_empty() {
        // 全部被跳过：保留块外原文，不新增块
        return Ok(CordisUpsert {
            content: outside,
            skipped,
        });
    }

    // 3. 组装：块外（清理 [] 占位）+ 托管块
    let base = strip_empty_sequence(&outside);
    let mut out = base;
    if !out.is_empty() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        if !out.ends_with("\n\n") {
            out.push('\n');
        }
    }
    out.push_str(DSH_MCP_BEGIN);
    out.push('\n');
    for chunk in &kept {
        out.push_str(chunk);
    }
    out.push_str(DSH_MCP_END);
    out.push('\n');

    Ok(CordisUpsert {
        content: out,
        skipped,
    })
}

/// 提取托管块内容（不含标记行）；标记缺失或顺序异常返回 None
pub fn extract_cordis_mcp_block(content: &str) -> Option<String> {
    let begin = content.find(DSH_MCP_BEGIN)?;
    let after_begin = begin + DSH_MCP_BEGIN.len();
    let rel_end = content[after_begin..].find(DSH_MCP_END)?;
    let body = &content[after_begin..after_begin + rel_end];
    Some(body.trim_start_matches(['\n', '\r']).to_string())
}

/// 删除托管块（含标记行），保留块外内容
fn remove_block(content: &str) -> Result<String> {
    let begin = content
        .find(DSH_MCP_BEGIN)
        .ok_or_else(|| anyhow!("托管块缺少起始标记"))?;
    let after_begin = begin + DSH_MCP_BEGIN.len();
    let rel_end = content[after_begin..]
        .find(DSH_MCP_END)
        .ok_or_else(|| anyhow!("托管块缺少结束标记（标记异常，拒绝写入）"))?;
    let end = after_begin + rel_end + DSH_MCP_END.len();
    // 连带结束标记后的换行一起去掉，避免重复迁移累积空行
    let tail = content[end..].trim_start_matches(['\n', '\r']);
    let mut out = String::from(&content[..begin]);
    out.push_str(tail);
    Ok(out.trim_end_matches(['\n', '\r']).to_string())
}

/// 收集块外所有 `- id: <x>` 的 id
fn collect_ids(content: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in content.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("- id:") {
            set.insert(rest.trim().trim_matches('\'').trim_matches('"').to_string());
        }
    }
    set
}

/// 按 `- id:` 切分片段 → [(id, chunk)]
fn split_entries(fragment: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in fragment.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("- id:") {
            let id = rest.trim().trim_matches('\'').trim_matches('"').to_string();
            out.push((id, format!("{line}\n")));
        } else if let Some(last) = out.last_mut() {
            last.1.push_str(line);
            last.1.push('\n');
        }
    }
    out
}

/// 移除文件末尾的 `[]` 空序列占位（dsh 的 cordis.yml 出厂内容就是 `[]`）
///
/// 只删「最后一条非空非注释行」且内容恰为 `[]` 的那一行，注释与其余内容保留。
fn strip_empty_sequence(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut last_code = None;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if !t.is_empty() && !t.starts_with('#') {
            last_code = Some(i);
        }
    }
    let Some(idx) = last_code else {
        // 全是注释/空行 → 原样保留（会以空行结尾则在组装时补）
        return content.trim_end_matches(['\n', '\r']).to_string();
    };
    if lines[idx].trim() != "[]" {
        return content.trim_end_matches(['\n', '\r']).to_string();
    }
    let mut kept: Vec<&str> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if i == idx {
            continue;
        }
        kept.push(line);
    }
    // 去掉尾部因删除产生的空行
    while kept.last().is_some_and(|l| l.trim().is_empty()) {
        kept.pop();
    }
    kept.join("\n")
}

/// 托管块内容 → 标准 mcpServers JSON（反向迁移用）
///
/// 只解析 `@deepseek-ai/dsh-mcp-client` 条目；`!!js` 表达式还原为 `{env:VAR}` 占位符，
/// 保证 JSON → cordis → JSON 往返语义一致。
pub fn cordis_block_to_json(block: &str) -> Result<String> {
    if block.trim().is_empty() {
        return Ok(json!({ "mcpServers": {} }).to_string());
    }
    let parsed: serde_yaml::Value =
        serde_yaml::from_str(block).context("cordis patch 托管块不是合法 YAML")?;
    let seq = match parsed {
        serde_yaml::Value::Sequence(s) => s,
        _ => return Err(anyhow!("cordis patch 托管块应为序列（YAML list）")),
    };

    let mut servers = Map::new();
    for item in seq {
        let map = match item {
            serde_yaml::Value::Mapping(m) => m,
            _ => continue,
        };
        let name = map
            .get(yaml_key("name"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if name != DSH_MCP_PLUGIN {
            continue;
        }
        let cfg = match map.get(yaml_key("config")) {
            Some(serde_yaml::Value::Mapping(m)) => m.clone(),
            _ => continue,
        };
        let server_name = cfg
            .get(yaml_key("serverName"))
            .and_then(|v| v.as_str())
            .unwrap_or("server")
            .to_string();

        let mut def = Map::new();
        if let Some(cmd) = cfg.get(yaml_key("command")).and_then(|v| v.as_str()) {
            def.insert("command".into(), json!(cmd));
        }
        if let Some(args) = cfg.get(yaml_key("args")) {
            if let serde_yaml::Value::Sequence(items) = args {
                let list: Vec<Value> = items
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| json!(s)))
                    .collect();
                if !list.is_empty() {
                    def.insert("args".into(), Value::Array(list));
                }
            }
        }
        if let Some(cwd) = cfg.get(yaml_key("cwd")).and_then(|v| v.as_str()) {
            def.insert("cwd".into(), json!(cwd));
        }
        if let Some(url) = cfg.get(yaml_key("url")).and_then(|v| v.as_str()) {
            def.insert("url".into(), json!(url));
        }
        for field in ["env", "headers"] {
            if let Some(serde_yaml::Value::Mapping(m)) = cfg.get(yaml_key(field)) {
                let mut obj = Map::new();
                for (k, v) in m {
                    let key = match k.as_str() {
                        Some(s) => s.to_string(),
                        None => continue,
                    };
                    obj.insert(key, yaml_scalar_to_json(v));
                }
                if !obj.is_empty() {
                    def.insert(field.into(), Value::Object(obj));
                }
            }
        }
        servers.insert(server_name, Value::Object(def));
    }

    Ok(serde_json::to_string_pretty(&json!({ "mcpServers": servers }))?)
}

fn yaml_key(k: &str) -> serde_yaml::Value {
    serde_yaml::Value::String(k.to_string())
}

/// YAML 标量 → JSON 值；`!!js` 表达式还原为 `{env:VAR}` 占位符
///
/// 实测（serde_yaml 0.9）: `!!js` 属于 YAML 标准 tag 命名空间（`tag:yaml.org,2002:js`），
/// 解析器**不会**把它包成 `Value::Tagged`，而是折叠成普通字符串。
/// 因此两条分支都要做 `process.env.X` / 模板字符串的还原。
fn yaml_scalar_to_json(v: &serde_yaml::Value) -> Value {
    match v {
        serde_yaml::Value::Tagged(t) => {
            let raw = t.value.as_str().unwrap_or_default();
            json!(js_to_env_placeholder(raw))
        }
        serde_yaml::Value::Null => Value::Null,
        serde_yaml::Value::Bool(b) => json!(b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                json!(i)
            } else if let Some(f) = n.as_f64() {
                json!(f)
            } else {
                json!(n.to_string())
            }
        }
        serde_yaml::Value::String(s) => json!(js_to_env_placeholder(s)),
        other => json!(format!("{other:?}")),
    }
}

/// `process.env.VAR` / `` `Bearer ${process.env.VAR}` `` → `{env:VAR}` 占位符
fn js_to_env_placeholder(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(var) = trimmed.strip_prefix("process.env.") {
        if is_ident(var) {
            return format!("{{env:{var}}}");
        }
    }
    // 模板字符串: `...${process.env.X}...`
    if trimmed.starts_with('`') && trimmed.ends_with('`') && trimmed.len() >= 2 {
        let inner = &trimmed[1..trimmed.len() - 1];
        let mut out = String::new();
        let mut rest = inner;
        while let Some(idx) = rest.find("${process.env.") {
            let (head, tail) = rest.split_at(idx);
            out.push_str(&head.replace("\\`", "`").replace("\\\\", "\\").replace("\\${", "${"));
            let after = &tail["${process.env.".len()..];
            match after.find('}') {
                Some(end) => {
                    let var = &after[..end];
                    out.push_str(&format!("{{env:{var}}}"));
                    rest = &after[end + 1..];
                }
                None => {
                    out.push_str(tail);
                    rest = "";
                }
            }
        }
        out.push_str(&rest.replace("\\`", "`").replace("\\\\", "\\").replace("\\${", "${"));
        return out;
    }
    raw.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "mcpServers": {
        "context7": {"command": "npx", "args": ["-y", "@upstash/context7-mcp@3.2.5"]},
        "obsidian": {"command": "mcp-server-filesystem.cmd", "args": ["C:\\Users\\me\\Obsidian Vault"]},
        "github": {"url": "https://api.githubcopilot.com/mcp/",
                   "headers": {"Authorization": "Bearer {env:GITHUB_PERSONAL_ACCESS_TOKEN}"}},
        "pg": {"command": "powershell.exe", "args": ["-File", "start.ps1"],
               "env": {"PG_URL": "{env:ZZ_POSTGRES_URL}", "MODE": "strict"}}
      }
    }"#;

    #[test]
    fn convert_produces_stdio_and_http_entries() {
        let (fragment, needs_review, _notes) = convert_json_mcp_to_cordis(SAMPLE);
        assert!(!needs_review);
        // stdio
        assert!(fragment.contains("- id: mcp-context7"));
        assert!(fragment.contains("transport: stdio"));
        assert!(fragment.contains("command: 'npx'"));
        assert!(fragment.contains("      - '-y'"));
        // 反斜杠路径必须被单引号保护
        assert!(fragment.contains(r"'C:\Users\me\Obsidian Vault'"));
        // http + header 占位符 → JS 模板字符串（单引号包裹，反引号不能裸起 YAML 标量）
        assert!(fragment.contains("- id: mcp-github"));
        assert!(fragment.contains("transport: streamable-http"));
        assert!(fragment
            .contains("Authorization: !!js '`Bearer ${process.env.GITHUB_PERSONAL_ACCESS_TOKEN}`'"));
        // env 整值占位符 → process.env
        assert!(fragment.contains("PG_URL: !!js 'process.env.ZZ_POSTGRES_URL'"));
        assert!(fragment.contains("MODE: 'strict'"));
    }

    #[test]
    fn convert_skips_disabled_servers() {
        let json = r#"{"mcpServers":{"a":{"command":"x","disabled":true},"b":{"command":"y"}}}"#;
        let (fragment, needs_review, notes) = convert_json_mcp_to_cordis(json);
        assert!(needs_review);
        assert!(notes.iter().any(|n| n.contains("disabled")));
        assert!(!fragment.contains("mcp-a"));
        assert!(fragment.contains("mcp-b"));
    }

    #[test]
    fn convert_normalizes_invalid_server_name() {
        let json = r#"{"mcpServers":{"my server.v2":{"command":"x"}}}"#;
        let (fragment, need_review, notes) = convert_json_mcp_to_cordis(json);
        assert!(need_review);
        assert!(notes.iter().any(|n| n.contains("归一化")));
        assert!(fragment.contains("serverName: 'my-server-v2'"));
    }

    #[test]
    fn convert_rejects_broken_json() {
        let (fragment, need_review, notes) = convert_json_mcp_to_cordis("{not json");
        assert!(fragment.is_empty());
        assert!(need_review);
        assert!(notes[0].contains("解析失败"));
    }

    #[test]
    fn upsert_appends_block_and_keeps_user_entries() {
        let existing = "\
# Your patch layer for this dsh profile
- id: ui-settings
  name: '@deepseek-ai/dsh-client-ui-settings'
  config:
    enabled: true
- id: llm-pi-ai
  name: '@deepseek-ai/dsh-llm-pi-ai'
  config:
    providers: {}
";
        let (fragment, _, _) = convert_json_mcp_to_cordis(SAMPLE);
        let out = upsert_cordis_mcp_block(existing, &fragment).unwrap();
        assert!(out.skipped.is_empty());
        // 用户条目与注释逐字保留
        assert!(out.content.contains("# Your patch layer for this dsh profile"));
        assert!(out.content.contains("id: llm-pi-ai"));
        assert!(out.content.contains("providers: {}"));
        // 托管块存在且只一个
        assert_eq!(out.content.matches(DSH_MCP_BEGIN).count(), 1);
        assert!(out.content.contains("id: mcp-context7"));
    }

    #[test]
    fn upsert_is_idempotent() {
        let (fragment, _, _) = convert_json_mcp_to_cordis(SAMPLE);
        let once = upsert_cordis_mcp_block("", &fragment).unwrap().content;
        let twice = upsert_cordis_mcp_block(&once, &fragment).unwrap().content;
        assert_eq!(once, twice, "重复迁移必须收敛到同一内容");
        assert_eq!(twice.matches(DSH_MCP_BEGIN).count(), 1);
    }

    #[test]
    fn upsert_replaces_stale_block_content() {
        let old = format!("{DSH_MCP_BEGIN}\n- id: mcp-old\n  name: '{DSH_MCP_PLUGIN}'\n{DSH_MCP_END}\n");
        let (fragment, _, _) = convert_json_mcp_to_cordis(
            r#"{"mcpServers":{"fresh":{"command":"x"}}}"#,
        );
        let out = upsert_cordis_mcp_block(&old, &fragment).unwrap();
        assert!(!out.content.contains("mcp-old"));
        assert!(out.content.contains("mcp-fresh"));
    }

    #[test]
    fn upsert_strips_empty_sequence_placeholder() {
        let existing = "# comment only file\n[]\n";
        let (fragment, _, _) = convert_json_mcp_to_cordis(r#"{"mcpServers":{"a":{"command":"x"}}}"#);
        let out = upsert_cordis_mcp_block(existing, &fragment).unwrap();
        assert!(!out.content.contains("[]"), "必须移除 [] 占位: {}", out.content);
        assert!(out.content.contains("# comment only file"));
        // 生成的 YAML 必须可解析
        let parsed: serde_yaml::Value = serde_yaml::from_str(&out.content).unwrap();
        assert!(parsed.is_sequence());
    }

    #[test]
    fn upsert_skips_ids_already_defined_outside_block() {
        let existing = "\
- id: mcp-github
  name: '@deepseek-ai/dsh-mcp-client'
  config:
    serverName: github
    transport: streamable-http
    url: 'https://example.com/mcp'
";
        let (fragment, _, _) = convert_json_mcp_to_cordis(SAMPLE);
        let out = upsert_cordis_mcp_block(existing, &fragment).unwrap();
        assert_eq!(out.skipped, vec!["github".to_string()]);
        // 不产生第二个 id: mcp-github
        assert_eq!(out.content.matches("id: mcp-github").count(), 1);
        // 其余 server 仍写入
        assert!(out.content.contains("id: mcp-context7"));
    }

    #[test]
    fn roundtrip_json_to_cordis_and_back() {
        let (fragment, _, _) = convert_json_mcp_to_cordis(SAMPLE);
        let json = cordis_block_to_json(&fragment).unwrap();
        let v: Value = serde_json::from_str(&json).unwrap();
        let servers = v["mcpServers"].as_object().unwrap();
        assert_eq!(servers.len(), 4);
        assert_eq!(servers["context7"]["command"], "npx");
        assert_eq!(servers["context7"]["args"][1], "@upstash/context7-mcp@3.2.5");
        assert_eq!(
            servers["github"]["headers"]["Authorization"],
            "Bearer {env:GITHUB_PERSONAL_ACCESS_TOKEN}"
        );
        assert_eq!(servers["pg"]["env"]["PG_URL"], "{env:ZZ_POSTGRES_URL}");
        assert_eq!(servers["pg"]["env"]["MODE"], "strict");
    }

    #[test]
    fn block_to_json_empty_is_empty_servers() {
        let json = cordis_block_to_json("").unwrap();
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["mcpServers"].as_object().unwrap().len(), 0);
    }

    #[test]
    fn extract_block_handles_markers() {
        let content = format!("a: 1\n{DSH_MCP_BEGIN}\n- id: mcp-x\n{DSH_MCP_END}\n");
        let block = extract_cordis_mcp_block(&content).unwrap();
        assert!(block.contains("mcp-x"));
        // 缺结束标记 → None
        assert!(extract_cordis_mcp_block(&format!("{DSH_MCP_BEGIN}\n- id: mcp-x\n")).is_none());
    }
}
