//! Agents 转换器 — Claude Code / Codex agents → Qoder subagent 格式
//!
//! Qoder subagent 官方格式（docs.qoder.com/zh/cli/subagent）:
//!   - Markdown 文件（.md），必须 YAML frontmatter 开头
//!   - frontmatter 字段: name / description / tools / disallowedTools /
//!     permissionMode / model / maxTurns / timeoutMins / isolation
//!   - 正文 = system prompt
//!
//! 输入兼容两种格式:
//!   - Claude: .md + YAML frontmatter（tools 是 YAML 列表）
//!   - Codex: .toml（tools 是数组 / developer_instructions 三引号块）
//!
//! 无法 1:1 的字段（如 Codex sandbox_mode）→ MANUAL 块

use crate::converter::manual_block::manual_block;

/// 转换单个 agent 文件内容
///
/// 返回 (转换后内容, 是否需要人工审查, 审查备注)
pub fn convert_agent(source_content: &str) -> (String, bool, Vec<String>) {
    let mut manual_notes = Vec::new();
    let mut needs_review = false;

    // 提取关键字段（兼容 YAML frontmatter 和 TOML 两种格式）
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut tools: Option<String> = None; // 逗号分隔
    let mut disallowed: Option<String> = None;
    let mut permission: Option<String> = None;
    let mut model: Option<String> = None;

    // 判断输入格式: TOML (name = "...") vs YAML frontmatter (---\nname: ...)
    let is_toml = source_content.contains("name =") && !source_content.trim_start().starts_with("---");

    if is_toml {
        // TOML 格式: 逐行解析顶层 key = value
        for line in source_content.lines() {
            let t = line.trim_start();
            if t.starts_with('#') || t.is_empty() {
                continue;
            }
            if let Some(v) = t.strip_prefix("name =") {
                name = Some(v.trim().trim_matches('"').to_string());
            } else if let Some(v) = t.strip_prefix("description =") {
                description = Some(v.trim().trim_matches('"').to_string());
            } else if let Some(v) = t.strip_prefix("model =") {
                model = Some(v.trim().trim_matches('"').to_string());
            } else if let Some(v) = t.strip_prefix("sandbox_mode =") {
                // sandbox_mode → permissionMode 近似映射
                permission = Some(match v.trim().trim_matches('"') {
                    "workspace-write" => "acceptEdits".into(),
                    "read-only" => "readOnly".into(),
                    _ => "default".into(),
                });
                manual_notes.push("sandbox_mode mapped to permissionMode (approx)".into());
            }
        }
        // TOML 数组: tools = ["Read", "Bash"]
        if let Some(arr) = extract_toml_array(source_content, "tools") {
            tools = Some(arr.join(","));
        }
        if let Some(arr) = extract_toml_array(source_content, "disallowed_tools") {
            disallowed = Some(arr.join(","));
        }
        // TOML agent 转换需审查（developer_instructions 语义）
        needs_review = true;
        manual_notes.push("developer_instructions preserved, sandbox semantics may differ".into());
    } else {
        // YAML frontmatter 格式
        let (frontmatter, body) = split_agent_frontmatter(source_content);
        if let Some(fm) = frontmatter {
            let mut in_tools = false;
            let mut in_disallowed = false;
            let mut tools_list: Vec<String> = Vec::new();
            let mut disallowed_list: Vec<String> = Vec::new();
            for line in fm.lines() {
                let t = line.trim_start();
                if let Some(v) = t.strip_prefix("name:") {
                    name = Some(v.trim().to_string());
                } else if let Some(v) = t.strip_prefix("description:") {
                    description = Some(v.trim().to_string());
                } else if let Some(v) = t.strip_prefix("model:") {
                    model = Some(v.trim().to_string());
                } else if let Some(v) = t.strip_prefix("permissionMode:") {
                    permission = Some(v.trim().to_string());
                } else if t.starts_with("tools:") && !t.starts_with("tools: ") {
                    in_tools = true;
                    in_disallowed = false;
                } else if t.starts_with("disallowedTools:") {
                    in_disallowed = true;
                    in_tools = false;
                } else if let Some(v) = t.strip_prefix("tools:") {
                    tools_list.push(v.trim().to_string());
                } else if in_tools {
                    if let Some(item) = t.strip_prefix('-') {
                        tools_list.push(item.trim().to_string());
                    }
                } else if in_disallowed {
                    if let Some(item) = t.strip_prefix('-') {
                        disallowed_list.push(item.trim().to_string());
                    }
                }
            }
            if !tools_list.is_empty() {
                tools = Some(tools_list.join(","));
            }
            if !disallowed_list.is_empty() {
                disallowed = Some(disallowed_list.join(","));
            }
        }
        let _ = body;
    }

    // 组装输出: YAML frontmatter + 正文（Qoder subagent 官方格式）
    let mut out = String::new();
    out.push_str("---\n");
    if let Some(n) = &name {
        out.push_str(&format!("name: {n}\n"));
    }
    if let Some(d) = &description {
        out.push_str(&format!("description: {d}\n"));
    }
    if let Some(t) = &tools {
        out.push_str(&format!("tools: {t}\n"));
    }
    if let Some(t) = &disallowed {
        out.push_str(&format!("disallowedTools: {t}\n"));
    }
    if let Some(p) = &permission {
        out.push_str(&format!("permissionMode: {p}\n"));
    }
    if let Some(m) = &model {
        out.push_str(&format!("model: {m}\n"));
    }
    out.push_str("---\n\n");

    // 正文 = system prompt（保留原 body / TOML developer_instructions）
    let body_text = if is_toml {
        extract_toml_instructions(source_content)
    } else {
        split_agent_frontmatter(source_content).1.unwrap_or("").trim().to_string()
    };
    if !body_text.is_empty() {
        out.push_str(&body_text);
        out.push('\n');
    }

    // 语义差异 → MANUAL 块（字段已保留为 frontmatter，但语义可能不完全等价）
    let mut caveats = Vec::new();
    if tools.is_some() {
        caveats.push("tools list preserved (verify tool names match Qoder)");
    }
    if permission.is_some() {
        caveats.push("permissionMode preserved (semantics may differ)");
    }
    if model.is_some() {
        caveats.push("model field preserved (Qoder may override)");
    }

    if !caveats.is_empty() {
        needs_review = true;
        let reason = "Agent fields were preserved in frontmatter, but their semantics may differ between the source platform and Qoder.";
        let guidance = format!(
            "Review the mapped agent fields below. Qoder supports these fields natively but enforces them differently from Claude Code / Codex.\n\nPreserved fields with potential semantic differences:\n- {}",
            caveats.join("\n- ")
        );
        out.push('\n');
        out.push_str(&manual_block(&reason.to_string(), &guidance));
        for c in caveats {
            manual_notes.push(c.to_string());
        }
    }

    (out, needs_review, manual_notes)
}

/// 拆分 agent frontmatter（同 SKILL.md 格式）
fn split_agent_frontmatter(content: &str) -> (Option<&str>, Option<&str>) {
    crate::converter::converters::skills::split_frontmatter(content)
}

/// 提取 TOML 格式的 developer_instructions（三引号块）
fn extract_toml_instructions(content: &str) -> String {
    let start = content.find("developer_instructions = \"\"\"");
    match start {
        Some(pos) => {
            let rest = &content[pos + "developer_instructions = \"\"\"".len()..];
            match rest.find("\"\"\"") {
                Some(end) => rest[..end].trim().to_string(),
                None => String::new(),
            }
        }
        None => String::new(),
    }
}

/// 提取 TOML 数组值: tools = ["Read", "Bash"] → ["Read", "Bash"]
fn extract_toml_array(content: &str, key: &str) -> Option<Vec<String>> {
    for line in content.lines() {
        let t = line.trim_start();
        if let Some(v) = t.strip_prefix(&format!("{key} =")) {
            let v = v.trim();
            if v.starts_with('[') && v.ends_with(']') {
                let inner = &v[1..v.len() - 1];
                return Some(
                    inner
                        .split(',')
                        .map(|s| s.trim().trim_matches('"').to_string())
                        .filter(|s| !s.is_empty())
                        .collect(),
                );
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_AGENT: &str = "---\nname: release-lead\ndescription: coordinates releases\ntools:\n  - Read\n  - Bash\ndisallowedTools:\n  - Write\npermissionMode: acceptEdits\n---\n\nCoordinate the release process. Check CI status first.\n";

    #[test]
    fn converts_agent_basics() {
        let (out, needs_review, _) = convert_agent(SAMPLE_AGENT);
        // Qoder 格式: YAML frontmatter
        assert!(out.starts_with("---\n"));
        assert!(out.contains("name: release-lead"));
        assert!(out.contains("description: coordinates releases"));
        assert!(out.contains("tools: Read,Bash"));
        assert!(out.contains("disallowedTools: Write"));
        assert!(out.contains("permissionMode: acceptEdits"));
        assert!(out.contains("Coordinate the release process."));
        assert!(needs_review);
    }

    #[test]
    fn downgraded_fields_generate_manual_block() {
        let (out, _, notes) = convert_agent(SAMPLE_AGENT);
        assert!(crate::converter::manual_block::contains_manual_block(&out));
        assert!(notes.iter().any(|n| n.contains("tools")));
        assert!(notes.iter().any(|n| n.contains("permissionMode")));
    }

    #[test]
    fn clean_agent_no_review() {
        let clean = "---\nname: simple\ndescription: minimal\n---\n\nJust do the thing.\n";
        let (out, needs_review, notes) = convert_agent(clean);
        assert!(!needs_review);
        assert!(notes.is_empty());
        assert!(out.contains("name: simple"));
        assert!(out.contains("Just do the thing."));
        // 无字段差异时不生成 MANUAL 块
        assert!(!crate::converter::manual_block::contains_manual_block(&out));
    }

    #[test]
    fn toml_agent_parses_name_and_description() {
        let toml = "name = \"mtkf-coder\"\ndescription = \"MTKF implementation agent\"\nmodel = \"gpt-5.6-sol\"\nsandbox_mode = \"workspace-write\"\ndeveloper_instructions = \"\"\"\n你是 MTKF Coder。\n\"\"\"\n";
        let (out, needs_review, notes) = convert_agent(toml);
        assert!(out.starts_with("---\n"));
        assert!(out.contains("name: mtkf-coder"));
        assert!(out.contains("description: MTKF implementation agent"));
        assert!(out.contains("model: gpt-5.6-sol"));
        assert!(out.contains("permissionMode: acceptEdits"));
        assert!(out.contains("你是 MTKF Coder"));
        // sandbox_mode → permissionMode 近似映射触发审查
        assert!(needs_review);
        assert!(notes.iter().any(|n| n.contains("sandbox_mode")));
    }

    #[test]
    fn toml_agent_extracts_instructions() {
        let toml = "name = \"a\"\ndeveloper_instructions = \"\"\"\nline1\nline2\n\"\"\"\n";
        let extracted = extract_toml_instructions(toml);
        assert_eq!(extracted, "line1\nline2");
    }

    #[test]
    fn toml_agent_extracts_tools_array() {
        let toml = "name = \"a\"\ntools = [\"Read\", \"Bash\", \"Grep\"]\ndisallowed_tools = [\"Write\"]\n";
        let (out, _, _) = convert_agent(toml);
        assert!(out.contains("tools: Read,Bash,Grep"));
        assert!(out.contains("disallowedTools: Write"));
    }
}
