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

/// 解析单行标记，返回标记内部文本（如 `MIGRATOR:migrator:workbuddy:instructions`）
fn marker_inner(line: &str, kind: &str) -> Option<String> {
    let t = line.trim();
    let rest = t.strip_prefix(&format!("<!-- {kind} "))?;
    let inner = rest.strip_suffix("-->")?.trim_end();
    if inner.starts_with(MARKER_PREFIX) {
        Some(inner.to_string())
    } else {
        None
    }
}

/// 收集文件中所有 MIGRATOR 块：`(标记内部文本, 起始字节, 结束字节)`
///
/// 行级配对（以 BEGIN/END 同名标记配对），容忍块中块与历史叠加的多个块。
/// 供 cutover 使用：旧契约的 `migrator:<source_path>` 块必须能被找到并升级，
/// 否则新 scope 会另起一块，文件里留下两个块。
fn migrator_block_ranges(content: &str) -> Vec<(String, usize, usize)> {
    let mut stack: Vec<(String, usize)> = Vec::new();
    let mut out: Vec<(String, usize, usize)> = Vec::new();
    let mut offset = 0usize;
    for line in content.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        if let Some(inner) = marker_inner(line, "BEGIN") {
            stack.push((inner, line_start));
        } else if let Some(inner) = marker_inner(line, "END")
            && let Some(pos) = stack.iter().rposition(|(s, _)| *s == inner)
        {
            let (s, start) = stack.remove(pos);
            out.push((s, start, offset));
        }
    }
    out.sort_by_key(|(_, s, _)| *s);
    out
}

/// 内容中是否存在任意 MIGRATOR 块（不限 scope），返回第一个块的内部标记文本
pub fn find_any_migrator_block(content: &str) -> Option<String> {
    migrator_block_ranges(content)
        .first()
        .map(|(s, _, _)| s.clone())
}

/// 在目标文件中更新托管块区域（幂等）
///
/// 规则:
/// 1. 无托管块 → 追加到文件末尾（first_write=true）
/// 2. 有托管块 → 只重写 BEGIN..END 之间区域，其余保留
/// 3. 标记异常（重复/缺半/顺序反）→ 返回错误，拒绝写入
/// 4. **cutover**: 当前 scope 不存在但文件里有旧契约 scope 的块
///    （形如 `migrator:<source_path>`）→ 就地升级为当前 scope，不追加第二个块
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

    // 首次写入（当前 scope 无块）:
    // cutover 优先 —— 若文件里已有旧契约 scope 的 MIGRATOR 块，就地升级它，
    // 绝不追加第二个块。旧契约 scope 形如 `migrator:migrator:~/.codex/AGENTS.md`
    // （v0.2 早期用 `migrator:<source_path>`），用户真实文件里大量存在；
    // 若不接管，每次 apply 都会新起一块，旧块内容永久陈旧且两块并存。
    if begin_count == 0 {
        let ranges = migrator_block_ranges(existing);
        if let Some((_, first_start, first_end)) = ranges.first().cloned() {
            let mut out = existing.to_string();
            // 先删掉第一个块之外的所有历史块（从后往前删，避免偏移失效）
            for (_, s, e) in ranges.iter().skip(1).rev() {
                out.replace_range(*s..*e, "");
            }
            // 再把第一个块就地重写为当前 scope（标记与内容都更新）
            let mut block = String::new();
            block.push_str(&begin);
            block.push('\n');
            block.push_str(new_content);
            if !new_content.ends_with('\n') {
                block.push('\n');
            }
            block.push_str(&end);
            block.push('\n');
            out.replace_range(first_start..first_end, &block);
            return Ok(ManagedBlockResult {
                content: out,
                // 不是首次写入，而是旧块升级：语义上块已存在
                first_write: false,
            });
        }

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

/// 判断某一行是否为托管块标记（BEGIN / END 任意 scope）
fn is_marker_line(line: &str) -> bool {
    let t = line.trim();
    (t.starts_with("<!-- BEGIN ") || t.starts_with("<!-- END ")) && t.contains(MARKER_PREFIX)
}

/// 剥离源内容中的托管块标记，返回 (干净内容, 剥离的标记数)
///
/// 契约 v1 规定「源内容中的标记需转义，防误认」。这里选择**整行剥离**而非转义：
/// 标记对目标平台没有语义，转义只会把 `\<!-- BEGIN ... -->` 这种噪音带到新文件里；
/// 剥离则让 A→B→A 的往返迁移结果干净。
///
/// 真实故障：`dsh → codex` 时 dsh `AGENTS.md` 里的
/// `<!-- BEGIN MIGRATOR:migrator:dsh:instructions -->` 被原样搬运，再被 codex
/// 目标包一层，产物出现 4 个标记（块中块）。反复来回迁移会无限套娃。
pub fn strip_markers(content: &str) -> (String, usize) {
    let mut removed = 0usize;
    let mut out = String::with_capacity(content.len());
    for line in content.split_inclusive('\n') {
        if is_marker_line(line) {
            removed += 1;
            continue;
        }
        out.push_str(line);
    }
    // 末行无换行符时 split_inclusive 仍能取到，无需额外处理
    (out, removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 回归（真机 dsh→codex 发现）: 源文件的托管块标记必须被剥离，正文完整保留
    #[test]
    fn strip_markers_removes_markers_and_keeps_body() {
        let src = "手写开头\n\
                   <!-- BEGIN MIGRATOR:migrator:dsh:instructions -->\n\
                   # 规则正文\n\
                   <!-- END MIGRATOR:migrator:dsh:instructions -->\n\
                   手写结尾\n";
        let (out, n) = strip_markers(src);
        assert_eq!(n, 2, "应剥离 2 个标记");
        assert!(!out.contains("MIGRATOR"), "不应残留任何标记: {out}");
        assert!(out.contains("手写开头"));
        assert!(out.contains("# 规则正文"));
        assert!(out.contains("手写结尾"));
    }

    /// 块中块（历史上已经嵌套过的文件）也必须一次剥干净
    #[test]
    fn strip_markers_removes_nested_blocks() {
        let src = "<!-- BEGIN MIGRATOR:migrator:codex:instructions -->\n\
                   <!-- BEGIN MIGRATOR:migrator:dsh:instructions -->\n\
                   正文\n\
                   <!-- END MIGRATOR:migrator:dsh:instructions -->\n\
                   <!-- END MIGRATOR:migrator:codex:instructions -->\n";
        let (out, n) = strip_markers(src);
        assert_eq!(n, 4);
        assert_eq!(out.trim(), "正文");
    }

    /// 不含标记的普通文本必须逐字不变（含正文里出现 MIGRATOR 这个词）
    #[test]
    fn strip_markers_is_noop_without_markers() {
        let src = "# 普通规则\n- 提到 MIGRATOR 这个词也不算标记\n<!-- 普通注释 -->\n";
        let (out, n) = strip_markers(src);
        assert_eq!(n, 0);
        assert_eq!(out, src);
    }

    /// 回归（真机发现）: 用户文件里存在旧契约 scope
    /// `migrator:<source_path>` 的块时，必须**就地升级**为新 scope，
    /// 不能追加第二个块。否则每次 apply 都新起一块、旧块内容永久陈旧。
    #[test]
    fn upsert_upgrades_legacy_scope_in_place() {
        let legacy = "\
手写A
<!-- BEGIN MIGRATOR:migrator:~/.codex/AGENTS.md -->
旧内容
<!-- END MIGRATOR:migrator:~/.codex/AGENTS.md -->
手写B
";
        let r = upsert_managed_block(legacy, "migrator:workbuddy:instructions", "新内容").unwrap();
        assert!(!r.first_write, "旧块升级不应标记为首次写入");
        assert_eq!(
            r.content.matches("BEGIN MIGRATOR").count(),
            1,
            "绝不能追加第二个块:\n{}",
            r.content
        );
        assert!(r.content.contains("migrator:workbuddy:instructions"));
        assert!(
            !r.content.contains("migrator:~/.codex/AGENTS.md"),
            "旧 scope 标记必须被替换"
        );
        assert!(r.content.contains("手写A") && r.content.contains("手写B"));
        assert!(r.content.contains("新内容") && !r.content.contains("旧内容"));
    }

    /// 历史上已经叠加了多个旧块的文件，要收敛成一个
    #[test]
    fn upsert_collapses_multiple_legacy_blocks() {
        let multi = "\
A
<!-- BEGIN MIGRATOR:migrator:src1 -->
1
<!-- END MIGRATOR:migrator:src1 -->
B
<!-- BEGIN MIGRATOR:migrator:src2 -->
2
<!-- END MIGRATOR:migrator:src2 -->
C
";
        let r = upsert_managed_block(multi, "migrator:t:k", "new").unwrap();
        assert_eq!(
            r.content.matches("BEGIN MIGRATOR").count(),
            1,
            "多个旧块应收敛为一个:\n{}",
            r.content
        );
        assert!(r.content.contains('A') && r.content.contains('B') && r.content.contains('C'));
        assert!(r.content.contains("new"));
    }

    #[test]
    fn find_any_migrator_block_reports_scope_and_handles_absent() {
        let c = "x\n<!-- BEGIN MIGRATOR:migrator:a -->\n1\n<!-- END MIGRATOR:migrator:a -->\n";
        assert_eq!(
            find_any_migrator_block(c).as_deref(),
            Some("MIGRATOR:migrator:a")
        );
        assert!(find_any_migrator_block("普通内容\n无标记\n").is_none());
    }

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
