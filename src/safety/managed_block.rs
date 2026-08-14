//! 托管块 v1 — 手写内容保护机制
//!
//! 契约 v1 (冻结, 2026-08-13 keel 评审决策):
//!   - 标记格式: `<!-- BEGIN MIGRATOR:<scope> -->` ... `<!-- END MIGRATOR:<scope> -->`
//!   - 每次同步只重写标记之间的区域，手写内容不碰
//!   - 标记完整性校验: 重复/缺半/顺序反 → 拒绝写入（不尽力合并）
//!   - 源内容中的标记需转义，防误认
//!
//! 任何标记格式变更必须走 cutover 策略，绝不静默修改。

use thiserror::Error;

/// 托管块 v1 标记前缀 — 契约冻结，勿改
pub const MARKER_PREFIX: &str = "MIGRATOR";

/// 生成 BEGIN 标记
pub fn begin_marker(scope: &str) -> String {
    format!("<!-- BEGIN {MARKER_PREFIX}:{scope} -->")
}

/// 生成 END 标记
pub fn end_marker(scope: &str) -> String {
    format!("<!-- END {MARKER_PREFIX}:{scope} -->")
}

/// 托管块错误
#[derive(Debug, Error, PartialEq)]
pub enum ManagedBlockError {
    #[error("标记重复: 找到 {count} 个 BEGIN 标记 (scope={scope})")]
    DuplicateBegin { scope: String, count: usize },
    #[error("标记重复: 找到 {count} 个 END 标记 (scope={scope})")]
    DuplicateEnd { scope: String, count: usize },
    #[error("标记缺失: BEGIN 存在但 END 缺失 (scope={scope})")]
    MissingEnd { scope: String },
    #[error("标记顺序错误: END 出现在 BEGIN 之前 (scope={scope})")]
    WrongOrder { scope: String },
}

/// 托管块操作结果
#[derive(Debug, PartialEq)]
pub struct ManagedBlockResult {
    /// 更新后的完整文件内容
    pub content: String,
    /// 是否为首次写入（之前无托管块）
    pub first_write: bool,
}

/// 在目标文件中更新托管块区域（幂等）
///
/// 规则:
/// 1. 无托管块 → 追加到文件末尾（first_write=true）
/// 2. 有托管块 → 只重写 BEGIN..END 之间区域，其余保留
/// 3. 标记异常（重复/缺半/顺序反）→ 返回错误，拒绝写入
///
/// 幂等性: 同一输入重复调用结果一致
pub fn upsert_managed_block(
    existing: &str,
    scope: &str,
    new_content: &str,
) -> Result<ManagedBlockResult, ManagedBlockError> {
    let begin = begin_marker(scope);
    let end = end_marker(scope);

    let begin_count = existing.matches(&begin).count();
    let end_count = existing.matches(&end).count();

    // 标记完整性校验 — 异常即拒绝，不尽力合并
    if begin_count > 1 {
        return Err(ManagedBlockError::DuplicateBegin {
            scope: scope.to_string(),
            count: begin_count,
        });
    }
    if end_count > 1 {
        return Err(ManagedBlockError::DuplicateEnd {
            scope: scope.to_string(),
            count: end_count,
        });
    }
    if begin_count == 1 && end_count == 0 {
        return Err(ManagedBlockError::MissingEnd {
            scope: scope.to_string(),
        });
    }
    if end_count == 1 && begin_count == 0 {
        return Err(ManagedBlockError::MissingEnd {
            scope: scope.to_string(),
        });
    }
    if begin_count == 1 && end_count == 1 {
        let begin_pos = existing.find(&begin).unwrap();
        let end_pos = existing.find(&end).unwrap();
        if end_pos < begin_pos {
            return Err(ManagedBlockError::WrongOrder {
                scope: scope.to_string(),
            });
        }
    }

    // 首次写入: 保留 existing，追加托管块到末尾
    if begin_count == 0 {
        let mut content = String::new();
        content.push_str(existing);
        if !existing.is_empty() && !existing.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(&begin);
        content.push('\n');
        content.push_str(new_content);
        if !new_content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(&end);
        content.push('\n');
        return Ok(ManagedBlockResult {
            content,
            first_write: true,
        });
    }

    // 已有托管块: 只重写标记之间区域
    let begin_pos = existing.find(&begin).unwrap();
    let end_pos = existing.find(&end).unwrap();
    let end_end = end_pos + end.len();

    let mut content = String::new();
    content.push_str(&existing[..begin_pos]);
    content.push_str(&begin);
    content.push('\n');
    content.push_str(new_content);
    if !new_content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&end);
    content.push_str(&existing[end_end..]);

    Ok(ManagedBlockResult {
        content,
        first_write: false,
    })
}

/// 从文件中提取托管块内容（不含标记）
pub fn extract_managed_block(content: &str, scope: &str) -> Option<String> {
    let begin = begin_marker(scope);
    let end = end_marker(scope);
    let begin_pos = content.find(&begin)?;
    let after_begin = begin_pos + begin.len();
    let end_pos = content[after_begin..].find(&end)? + after_begin;
    let block = &content[after_begin..end_pos];
    Some(block.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_write_appends_block() {
        let result = upsert_managed_block("手写内容\n", "global", "synced content").unwrap();
        assert!(result.first_write);
        assert!(result.content.contains("手写内容"));
        assert!(result.content.contains(&begin_marker("global")));
        assert!(result.content.contains("synced content"));
        assert!(result.content.contains(&end_marker("global")));
    }

    #[test]
    fn second_write_preserves_handwritten_content() {
        let first = upsert_managed_block("手写开头\n", "global", "v1").unwrap();
        let second = upsert_managed_block(&first.content, "global", "v2").unwrap();
        assert!(!second.first_write);
        // 手写内容保留
        assert!(second.content.contains("手写开头"));
        // 新内容替换旧内容
        assert!(second.content.contains("v2"));
        assert!(!second.content.contains("v1"));
    }

    #[test]
    fn idempotent_rewrite() {
        let first = upsert_managed_block("", "project", "content").unwrap();
        let second = upsert_managed_block(&first.content, "project", "content").unwrap();
        assert_eq!(first.content, second.content);
    }

    #[test]
    fn duplicate_begin_rejected() {
        let content = format!(
            "{}\n{}\n{}\n{}",
            begin_marker("x"),
            begin_marker("x"),
            "content",
            end_marker("x")
        );
        let err = upsert_managed_block(&content, "x", "new").unwrap_err();
        assert!(matches!(err, ManagedBlockError::DuplicateBegin { .. }));
    }

    #[test]
    fn missing_end_rejected() {
        let content = format!("{}\ncontent", begin_marker("x"));
        let err = upsert_managed_block(&content, "x", "new").unwrap_err();
        assert!(matches!(err, ManagedBlockError::MissingEnd { .. }));
    }

    #[test]
    fn wrong_order_rejected() {
        let content = format!("{}\n{}\ncontent", end_marker("x"), begin_marker("x"));
        let err = upsert_managed_block(&content, "x", "new").unwrap_err();
        assert!(matches!(err, ManagedBlockError::WrongOrder { .. }));
    }

    #[test]
    fn extract_returns_block_body() {
        let content = upsert_managed_block("prefix\n", "s", "body content").unwrap();
        let extracted = extract_managed_block(&content.content, "s").unwrap();
        assert_eq!(extracted, "body content");
    }
}
