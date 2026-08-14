//! Agents 转换器 — .claude/agents/*.md → Qoder 格式
//!
//! Qoder 的 agents 格式与 Claude 不完全兼容（参考 migrate-to-codex 的映射表）:
//! - `tools` / `disallowedTools` → 降级为 developer_instructions 中的 prompt guidance
//! - `permissionMode` → 部分映射（仅 acceptEdits/readOnly）
//! - `model` + `effort` → 部分映射
//! 无法 1:1 的部分 → MANUAL 块

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
    let mut has_tools = false;
    let mut has_permission = false;
    let mut has_model = false;

    // 判断输入格式: TOML (name = "...") vs YAML frontmatter (name: "...")
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
            } else if t.starts_with("model_reasoning_effort =") || t.starts_with("model =") {
                has_model = true;
            } else if t.starts_with("sandbox_mode =") || t.starts_with("permission_mode =") {
                has_permission = true;
            }
        }
        // TOML 中的 developer_instructions 即 body
        let body_text = extract_toml_instructions(source_content);
        needs_review = !body_text.is_empty(); // TOML agent 转换到 Qoder 需审查 developer_instructions 语义
        if needs_review {
            manual_notes.push("developer_instructions preserved, sandbox semantics may differ".into());
        }
    } else {
        // YAML frontmatter 格式（原有逻辑）
        let (frontmatter, body) = split_agent_frontmatter(source_content);
        if let Some(fm) = frontmatter {
            for line in fm.lines() {
                let t = line.trim_start();
                if let Some(v) = t.strip_prefix("name:") {
                    name = Some(v.trim().to_string());
                } else if let Some(v) = t.strip_prefix("description:") {
                    description = Some(v.trim().to_string());
                } else if t.starts_with("tools:") || t.starts_with("disallowedTools:") {
                    has_tools = true;
                } else if t.starts_with("permissionMode:") {
                    has_permission = true;
                } else if t.starts_with("model:") {
                    has_model = true;
                }
            }
        }
        // body 保留
        let _ = body;
    }

    // 组装输出: TOML 风格 + developer_instructions 承载 prompt guidance
    let mut out = String::new();
    out.push_str("# Qoder agent (auto-migrated)\n\n");

    if let Some(n) = &name {
        out.push_str(&format!("name = \"{n}\"\n"));
    }
    if let Some(d) = &description {
        out.push_str(&format!("description = \"{d}\"\n"));
    }

    // developer_instructions: 保留原 body / 原 TOML instructions 作为指令
    let body_text = if is_toml {
        extract_toml_instructions(source_content)
    } else {
        split_agent_frontmatter(source_content).1.unwrap_or("").trim().to_string()
    };
    if !body_text.is_empty() {
        out.push_str("developer_instructions = \"\"\"\n");
        out.push_str(&body_text);
        out.push_str("\n\"\"\"\n");
    }

    // 降级字段 → MANUAL 块
    let mut downgraded = Vec::new();
    if has_tools {
        downgraded.push("tools/disallowedTools");
    }
    if has_permission {
        downgraded.push("permissionMode");
    }
    if has_model {
        downgraded.push("model");
    }

    if !downgraded.is_empty() {
        needs_review = true;
        let reason = format!(
            "Agent fields `{}` were preserved as prompt guidance, not enforced behavior in Qoder.",
            downgraded.join("`, `")
        );
        let guidance = format!(
            "Qoder does not enforce fine-grained agent permissions like Claude Code. Review and configure via Qoder sandbox_mode / permissions, or accept the prompt-guidance fallback.\n\nDowngraded fields:\n- {}",
            downgraded.join("\n- ")
        );
        out.push('\n');
        out.push_str(&manual_block(&reason, &guidance));
        manual_notes.push(format!(
            "agent fields downgraded: {}",
            downgraded.join(", ")
        ));
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

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_AGENT: &str = "---\nname: release-lead\ndescription: coordinates releases\ntools:\n  - Read\n  - Bash\ndisallowedTools:\n  - Write\npermissionMode: acceptEdits\n---\n\nCoordinate the release process. Check CI status first.\n";

    #[test]
    fn converts_agent_basics() {
        let (out, needs_review, _) = convert_agent(SAMPLE_AGENT);
        assert!(out.contains("name = \"release-lead\""));
        assert!(out.contains("description = \"coordinates releases\""));
        assert!(out.contains("developer_instructions"));
        assert!(out.contains("Coordinate the release process."));
        assert!(needs_review);
    }

    #[test]
    fn downgraded_fields_generate_manual_block() {
        let (out, _, notes) = convert_agent(SAMPLE_AGENT);
        assert!(crate::converter::manual_block::contains_manual_block(&out));
        assert!(notes.iter().any(|n| n.contains("tools/disallowedTools")));
        assert!(notes.iter().any(|n| n.contains("permissionMode")));
    }

    #[test]
    fn clean_agent_no_review() {
        let clean = "---\nname: simple\ndescription: minimal\n---\n\nJust do the thing.\n";
        let (out, needs_review, notes) = convert_agent(clean);
        assert!(!needs_review);
        assert!(notes.is_empty());
        assert!(out.contains("name = \"simple\""));
        assert!(out.contains("Just do the thing."));
    }

    #[test]
    fn toml_agent_parses_name_and_description() {
        let toml = "name = \"mtkf-coder\"\ndescription = \"MTKF implementation agent\"\nmodel = \"gpt-5.6-sol\"\nsandbox_mode = \"workspace-write\"\ndeveloper_instructions = \"\"\"\n你是 MTKF Coder。\n\"\"\"\n";
        let (out, needs_review, notes) = convert_agent(toml);
        assert!(out.contains("name = \"mtkf-coder\""));
        assert!(out.contains("description = \"MTKF implementation agent\""));
        assert!(out.contains("你是 MTKF Coder"));
        // model/sandbox_mode 触发降级审查
        assert!(needs_review);
        assert!(notes.iter().any(|n| n.contains("developer_instructions")));
    }

    #[test]
    fn toml_agent_extracts_instructions() {
        let toml = "name = \"a\"\ndeveloper_instructions = \"\"\"\nline1\nline2\n\"\"\"\n";
        let extracted = extract_toml_instructions(toml);
        assert_eq!(extracted, "line1\nline2");
    }
}
