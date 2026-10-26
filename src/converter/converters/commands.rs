//! Commands 转换器 — Claude Code slash 命令 → 单文件 Skill（SKILL.md）
//!
//! Claude 命令格式（.claude/commands/<name>.md）:
//!   - YAML frontmatter（description / argument-hint / allowed-tools 等）
//!   - 正文 = 命令提示词
//!
//! 输出为可被目标平台识别的单文件 skill:
//!   - YAML frontmatter: name（文件名）+ description（原样保留或取正文首行）
//!   - 正文 = 原命令提示词
//!
//! 契约: description 缺失时自动从正文首行生成，并标记需人工审查。

use crate::converter::converters::skills::split_frontmatter;

/// 转换单个 command 文件内容
///
/// name 取自源文件名（不含扩展名），如 review.md → "review"
/// 返回 (转换后内容, 是否需要人工审查, 审查备注)
pub fn convert_command(source_content: &str, name: &str) -> (String, bool, Vec<String>) {
    let mut manual_notes = Vec::new();
    let mut needs_review = false;

    // 提取原 frontmatter 的 description（Claude command 可带 description）
    let original_description = split_frontmatter(source_content).0.and_then(|fm| {
        fm.lines()
            .find_map(|l| l.trim_start().strip_prefix("description:"))
            .map(|v| v.trim().trim_matches('"').to_string())
    });

    // 正文 = frontmatter 之后的部分（保留原格式）
    let (_, body) = split_frontmatter(source_content);
    let body_text = body.unwrap_or(source_content).trim().to_string();

    // description 缺失 → 取正文首行（≤80 字符），标记审查
    let description = match original_description {
        Some(d) if !d.is_empty() => d,
        _ => {
            let first_line = body_text
                .lines()
                .next()
                .map(|l| l.trim().trim_start_matches(['#', ' ', '>']))
                .filter(|l| !l.is_empty())
                .unwrap_or(name)
                .chars()
                .take(80)
                .collect::<String>();
            manual_notes.push(format!(
                "description generated from first line ({}); review",
                first_line
            ));
            needs_review = true;
            first_line
        }
    };

    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("name: {name}\n"));
    out.push_str(&format!("description: {description}\n"));
    out.push_str("---\n\n");
    if !body_text.is_empty() {
        out.push_str(&body_text);
        out.push('\n');
    }

    (out, needs_review, manual_notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_command_with_description() {
        let cmd =
            "---\ndescription: 审查代码质量\n---\n\n请审查当前 diff 的代码质量，给出改进建议。\n";
        let (out, needs_review, notes) = convert_command(cmd, "review");
        assert!(out.starts_with("---\n"));
        assert!(out.contains("name: review"));
        assert!(out.contains("description: 审查代码质量"));
        assert!(out.contains("请审查当前 diff 的代码质量"));
        assert!(!needs_review);
        assert!(notes.is_empty());
    }

    #[test]
    fn generates_description_from_first_line() {
        let cmd = "# 快速生成单元测试\n\nWrite tests for the changed files.\n";
        let (out, needs_review, notes) = convert_command(cmd, "test-gen");
        assert!(out.contains("name: test-gen"));
        assert!(out.contains("description: 快速生成单元测试"));
        assert!(needs_review);
        assert!(notes.iter().any(|n| n.contains("description generated")));
    }

    #[test]
    fn keeps_body_verbatim() {
        let cmd = "---\ndescription: d\n---\nline1\nline2\n";
        let (out, _, _) = convert_command(cmd, "foo");
        assert!(out.contains("line1\nline2"));
    }
}
