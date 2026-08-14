//! MANUAL MIGRATION REQUIRED 块 v1 — 语义降级提示
//!
//! 来源: migrate-to-codex 的 `## MANUAL MIGRATION REQUIRED` 块
//! 契约 v1 (冻结): 当转换存在语义差异（Partial）或源行为在目标平台无等价物（Unsupported）时，
//! 生成 MANUAL 块写入目标文件，指导用户人工审查。
//!
//! 格式:
//! ```md
//! ## MANUAL MIGRATION REQUIRED
//!
//! <原因说明>
//!
//! <原始内容或指引>
//! ```

/// MANUAL 块标题 — 契约 v1
pub const MANUAL_BLOCK_TITLE: &str = "## MANUAL MIGRATION REQUIRED";

/// 生成 MANUAL 块
pub fn manual_block(reason: &str, guidance: &str) -> String {
    let mut out = String::new();
    out.push_str(MANUAL_BLOCK_TITLE);
    out.push('\n');
    out.push('\n');
    out.push_str(reason.trim());
    out.push('\n');
    out.push('\n');
    out.push_str(guidance.trim());
    out.push('\n');
    out
}

/// 检测内容中是否包含 MANUAL 块
pub fn contains_manual_block(content: &str) -> bool {
    content.contains(MANUAL_BLOCK_TITLE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_block_format() {
        let block = manual_block(
            "Claude `allowed-tools` was preserved as prompt guidance, not a permission boundary.",
            "You're allowed to use these tools:\n- Read\n- Bash",
        );
        assert!(block.starts_with(MANUAL_BLOCK_TITLE));
        assert!(block.contains("prompt guidance"));
        assert!(block.contains("You're allowed to use these tools:"));
        assert!(block.contains("- Read"));
    }

    #[test]
    fn detect_manual_block() {
        let content = format!("# Synced\n\n{}\n\nrest of file", manual_block("reason", "guide"));
        assert!(contains_manual_block(&content));
        assert!(!contains_manual_block("plain content"));
    }
}
