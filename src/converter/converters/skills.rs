//! Skills 转换器 — .claude/skills/<name>/SKILL.md → Qoder skills
//!
//! Qoder 与 Claude 的 SKILL.md 格式基本兼容（AGENTS 标准），
//! 但 `allowed-tools` / `model` / `hooks` 等字段在目标平台无等价物 → 降级为 prompt guidance + MANUAL 块

use crate::converter::manual_block::manual_block;

/// Skill frontmatter 中需要降级的字段（Qoder 无等价物）
const DOWNGRADE_FIELDS: &[&str] = &[
    "allowed-tools",
    "model",
    "effort",
    "disable-model-invocation",
    "argument-hint",
    "hooks",
    "paths",
    "shell",
];

/// 转换单个 SKILL.md 内容
///
/// 返回 (转换后内容, 是否需要人工审查, 审查备注)
pub fn convert_skill(source_content: &str) -> (String, bool, Vec<String>) {
    let mut manual_notes = Vec::new();

    // 1. 解析 frontmatter（--- 包裹的 YAML 块）
    let (frontmatter, body) = split_frontmatter(source_content);

    // 2. 检查需要降级的字段
    let mut downgraded = Vec::new();
    if let Some(fm) = frontmatter {
        for field in DOWNGRADE_FIELDS {
            if fm.contains(field) {
                downgraded.push(*field);
            }
        }
    }

    // 3. 组装输出：保留 frontmatter（去除降级字段及其缩进子行）+ body + MANUAL 块
    let mut out = String::new();
    if let Some(fm) = frontmatter {
        // 过滤掉降级字段所在的行及其缩进子行（YAML 列表项）
        let mut skip_indented = false;
        let filtered: Vec<&str> = fm
            .lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                // 缩进子行（YAML 值/列表项）跟随上一个被删字段
                if skip_indented && line.starts_with(' ') && !trimmed.is_empty() {
                    return false;
                }
                skip_indented = false;
                let is_downgraded = downgraded
                    .iter()
                    .any(|f| trimmed.starts_with(&format!("{f}:")) || trimmed.starts_with(&format!("{f} :")));
                if is_downgraded {
                    skip_indented = true;
                    return false;
                }
                true
            })
            .collect();
        out.push_str("---\n");
        out.push_str(&filtered.join("\n"));
        out.push_str("\n---\n");
    }

    // body 原样保留
    if let Some(b) = body {
        out.push_str(b);
        if !b.ends_with('\n') {
            out.push('\n');
        }
    }

    // 4. 降级字段生成 MANUAL 块
    if !downgraded.is_empty() {
        let reason = format!(
            "Skill fields `{}` were preserved as prompt guidance, not enforced behavior in Qoder.",
            downgraded.join("`, `")
        );
        let guidance = format!(
            "These fields have no direct equivalent in Qoder. Review and rewrite them as prompt instructions in the skill body:\n\n- {}",
            downgraded.join("\n- ")
        );
        out.push('\n');
        out.push_str(&manual_block(&reason, &guidance));
        manual_notes.push(format!(
            "fields downgraded: {}",
            downgraded.join(", ")
        ));
    }

    let needs_review = !downgraded.is_empty();
    (out, needs_review, manual_notes)
}

/// 从源内容中拆分 frontmatter 和正文
///
/// 返回 (frontmatter 内部内容, 正文)。无 frontmatter 时返回 (None, 全文)
pub fn split_frontmatter(content: &str) -> (Option<&str>, Option<&str>) {
    if !content.starts_with("---") {
        return (None, Some(content));
    }
    let rest = &content[3..];
    match rest.find("\n---") {
        Some(pos) => {
            let fm = &rest[..pos];
            let body_start = pos + 4; // skip "\n---"
            let body = &rest[body_start..];
            let body = body.trim_start_matches('\n');
            (Some(fm), Some(body))
        }
        None => (None, Some(content)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_SKILL: &str = "---\nname: my-skill\ndescription: does something\nallowed-tools:\n  - Read\n  - Bash\n---\n\n# My Skill\n\nDo the thing.\n";

    #[test]
    fn splits_frontmatter() {
        let (fm, body) = split_frontmatter(SAMPLE_SKILL);
        assert!(fm.unwrap().contains("name: my-skill"));
        assert!(body.unwrap().contains("# My Skill"));
    }

    #[test]
    fn no_frontmatter_returns_full_body() {
        let (fm, body) = split_frontmatter("just body");
        assert!(fm.is_none());
        assert_eq!(body.unwrap(), "just body");
    }

    #[test]
    fn converts_skill_preserves_body() {
        let (out, needs_review, _) = convert_skill(SAMPLE_SKILL);
        assert!(out.contains("# My Skill"));
        assert!(out.contains("Do the thing."));
        assert!(needs_review); // allowed-tools 被降级
    }

    #[test]
    fn downgraded_field_removed_from_frontmatter() {
        let (out, _, _) = convert_skill(SAMPLE_SKILL);
        // allowed-tools 行应被移除
        assert!(!out.contains("allowed-tools:"));
        // 但 name/description 保留
        assert!(out.contains("name: my-skill"));
        assert!(out.contains("description: does something"));
    }

    #[test]
    fn downgraded_field_children_removed() {
        let (out, _, _) = convert_skill(SAMPLE_SKILL);
        // allowed-tools 的缩进子行（- Read / - Bash）也应被移除
        assert!(!out.contains("- Read"));
        assert!(!out.contains("- Bash"));
    }

    #[test]
    fn manual_block_appended_when_downgraded() {
        let (out, _, _) = convert_skill(SAMPLE_SKILL);
        assert!(crate::converter::manual_block::contains_manual_block(&out));
    }

    #[test]
    fn clean_skill_no_review_needed() {
        let clean = "---\nname: clean\ndescription: nothing special\n---\n\nBody text.\n";
        let (out, needs_review, notes) = convert_skill(clean);
        assert!(!needs_review);
        assert!(notes.is_empty());
        assert!(out.contains("name: clean"));
    }
}
