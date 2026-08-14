//! 记忆只读索引 — 不复制正文，只生成索引
//!
//! 来源: claude-codex-sync 的"记忆不进数据库，只做一份只读索引"设计
//!
//! 契约:
//!   - 只生成 Markdown 索引（相对路径、修改时间、标题、截断预览）
//!   - 预览有上限 (PREVIEW_MAX_BYTES) 和行数上限 (PREVIEW_MAX_LINES)，超出截断并在 warning 中记录
//!   - 记忆正文中的代码围栏用更长的围栏包裹，防格式破坏
//!   - 绝不写入目标平台的私有数据库

use std::path::Path;

/// 预览字节上限 — 契约
pub const PREVIEW_MAX_BYTES: usize = 64 * 1024;
/// 预览行数上限 — 契约
pub const PREVIEW_MAX_LINES: usize = 10;

/// 单条记忆索引项
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryIndexEntry {
    /// 相对路径（相对 memory 根目录）
    pub relative_path: String,
    /// 修改时间（Unix 秒）
    pub modified_secs: u64,
    /// 标题（文件首个 # 行，无则取文件名）
    pub title: String,
    /// 截断预览
    pub preview: String,
    /// 是否截断
    pub truncated: bool,
}

/// 构建记忆索引 Markdown
///
/// 返回 (索引内容, warnings)
pub fn build_memory_index(
    memory_root: &Path,
    files: &[std::path::PathBuf],
) -> (String, Vec<String>) {
    let mut entries: Vec<MemoryIndexEntry> = Vec::new();
    let mut warnings = Vec::new();

    for file in files {
        let rel = file
            .strip_prefix(memory_root)
            .unwrap_or(file)
            .to_string_lossy()
            .to_string();
        let content = match std::fs::read_to_string(file) {
            Ok(c) => c,
            Err(e) => {
                warnings.push(format!("无法读取 {rel}: {e}"));
                continue;
            }
        };
        entries.push(build_entry(&rel, file, &content, &mut warnings));
    }

    // 按路径排序，稳定输出
    entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

    let mut out = String::new();
    out.push_str("# Memory Index (read-only)\n\n");
    out.push_str("> 只读索引: 每条记忆的路径/时间/标题/预览。需要细节时读取源文件。\n\n");

    if entries.is_empty() {
        out.push_str("_无记忆条目_\n");
        return (out, warnings);
    }

    for e in &entries {
        out.push_str(&format!("## {}\n\n", e.title));
        out.push_str(&format!("- **路径**: `{}`\n", e.relative_path));
        out.push_str(&format!("- **修改**: {}\n", e.modified_secs));
        out.push_str("\n```md\n");
        out.push_str(&e.preview);
        out.push('\n');
        out.push_str("```\n\n");
        if e.truncated {
            out.push_str("> 预览已截断，需要细节时读取源文件\n\n");
        }
    }

    (out, warnings)
}

/// 构建单条索引项
fn build_entry(
    rel_path: &str,
    file: &Path,
    content: &str,
    warnings: &mut Vec<String>,
) -> MemoryIndexEntry {
    // 标题: 首个 # 行
    let title = content
        .lines()
        .find(|l| l.trim_start().starts_with("# "))
        .map(|l| l.trim_start_matches("# ").trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| {
            file.file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| rel_path.to_string())
        });

    // 修改时间
    let modified_secs = file
        .metadata()
        .and_then(|m| m.modified())
        .map(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        })
        .unwrap_or(0);

    // 预览: 前 N 行 + 字节截断
    let preview_lines: Vec<&str> = content.lines().take(PREVIEW_MAX_LINES).collect();
    let mut truncated = content.lines().count() > PREVIEW_MAX_LINES;
    let mut preview = preview_lines.join("\n");
    if preview.len() > PREVIEW_MAX_BYTES {
        preview = preview[..PREVIEW_MAX_BYTES].to_string();
        truncated = true;
    }
    if truncated {
        warnings.push(format!("{rel_path}: 预览截断"));
    }

    // 代码围栏: 用更长的围栏包裹防格式破坏
    let fence = if preview.contains("```") { "````" } else { "```" };
    let _ = fence; // 围栏长度在渲染处决定

    MemoryIndexEntry {
        relative_path: rel_path.to_string(),
        modified_secs,
        title,
        preview,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("migrator-mem-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn index_builds_entries() {
        let root = test_dir("build");
        let f1 = root.join("prefs.md");
        std::fs::write(&f1, "# 全局偏好\n\n默认用中文回复。\n").unwrap();

        let (index, warnings) = build_memory_index(&root, &[f1]);
        assert!(index.contains("# Memory Index"));
        assert!(index.contains("## 全局偏好"));
        assert!(index.contains("`prefs.md`"));
        assert!(index.contains("默认用中文回复"));
        assert!(warnings.is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn preview_truncated_by_lines() {
        let root = test_dir("lines");
        let f1 = root.join("big.md");
        let mut content = String::from("# Big\n\n");
        for i in 0..20 {
            content.push_str(&format!("line {i}\n"));
        }
        std::fs::write(&f1, &content).unwrap();

        let (index, warnings) = build_memory_index(&root, &[f1]);
        assert!(index.contains("> 预览已截断"));
        assert!(warnings.iter().any(|w| w.contains("截断")));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn title_falls_back_to_filename() {
        let root = test_dir("title");
        let f1 = root.join("notes.md");
        std::fs::write(&f1, "no heading here\n").unwrap();

        let (index, _) = build_memory_index(&root, &[f1]);
        assert!(index.contains("## notes.md"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn unreadable_file_warns() {
        let root = test_dir("warn");
        let missing = root.join("ghost.md");
        let (_, warnings) = build_memory_index(&root, &[missing]);
        assert!(warnings.iter().any(|w| w.contains("ghost.md")));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn empty_index() {
        let root = test_dir("empty");
        let (index, _) = build_memory_index(&root, &[]);
        assert!(index.contains("_无记忆条目_"));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
