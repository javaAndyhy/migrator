//! 布局驱动目标适配器 — 写入目标平台配置
//!
//! 平台差异 = TargetLayout（instructions 文件名 / MCP 路径 / 配置根目录）
//! 新增平台 = 在 layout.rs 定义一个新 Layout，无需改转换逻辑

use super::{TargetAdapter, TargetWriteResult};
use crate::converter::mapping::ConversionResult;
use crate::target::layout::TargetLayout;
use std::path::{Path, PathBuf};

/// 去掉源平台路径前缀（.claude/ 或 ~/.codex/），返回相对路径
fn strip_source_prefix(source_path: &str) -> &str {
    source_path
        .trim_start_matches(".claude/")
        .trim_start_matches("~/.codex/")
        .trim_start_matches("~/.claude/")
}

/// 通用布局驱动目标适配器
pub struct LayoutTarget {
    /// 目标平台布局
    layout: TargetLayout,
    /// 用户级配置目录
    user_dir: PathBuf,
    /// 项目级配置目录
    project_dir: PathBuf,
}

impl LayoutTarget {
    pub fn new(layout: TargetLayout, user_dir: impl Into<PathBuf>, project_dir: impl Into<PathBuf>) -> Self {
        Self {
            layout,
            user_dir: user_dir.into(),
            project_dir: project_dir.into(),
        }
    }

    /// 从当前目录按布局探测目标位置
    pub fn detect(layout: TargetLayout, current_dir: &Path) -> Self {
        Self::new(
            layout.clone(),
            layout.user_root(),
            layout.project_root(current_dir),
        )
    }

    fn ensure_dir(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }
}

/// Qoder 目标适配器（便捷别名，向后兼容）
pub type QoderTarget = LayoutTarget;

impl QoderTarget {
    pub fn qoder_new(user_dir: impl Into<PathBuf>, project_dir: impl Into<PathBuf>) -> Self {
        Self::new(TargetLayout::qoder(), user_dir, project_dir)
    }

    pub fn qoder_detect(current_dir: &Path) -> Self {
        Self::detect(TargetLayout::qoder(), current_dir)
    }
}

impl TargetAdapter for LayoutTarget {
    fn name(&self) -> &'static str {
        self.layout.name
    }

    fn target_path(&self, source_path: &str) -> PathBuf {
        let rel = strip_source_prefix(source_path);
        let root = &self.project_dir;

        if source_path.ends_with("CLAUDE.md") || source_path.ends_with("AGENTS.md") {
            // 指令文件 → 平台自己的文件名（AGENTS.md / LINGMA.md）
            root.join(self.layout.instructions_file)
        } else if source_path.contains(".mcp.json")
            || source_path.contains("config.toml")
            || source_path.contains("mcp")
        {
            // MCP 配置 → 平台自己的 MCP 文件
            root.join(self.layout.mcp_file)
        } else if let Some(rest) = rel.strip_prefix("skills/") {
            root.join(self.layout.skills_dir).join(rest)
        } else if let Some(rest) = rel.strip_prefix("agents/") {
            root.join(self.layout.agents_dir).join(rest)
        } else if rel.contains("memor") {
            // memory 索引输出到 <root>/<memory_dir>/README.md
            root.join(self.layout.memory_dir).join("README.md")
        } else {
            root.join(rel)
        }
    }

    fn write(&self, path: &Path, result: &ConversionResult) -> std::io::Result<TargetWriteResult> {
        let created = !path.exists();
        self.ensure_dir(path)?;
        crate::safety::atomic::atomic_write(path, &result.content)?;
        Ok(TargetWriteResult {
            path: path.to_path_buf(),
            created,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn qoder_target() -> LayoutTarget {
        LayoutTarget::new(TargetLayout::qoder(), "~/.qoder", "./.qoder")
    }

    #[test]
    fn qoder_target_path_for_instructions() {
        let t = qoder_target();
        assert_eq!(
            t.target_path(".claude/CLAUDE.md"),
            PathBuf::from("./.qoder/AGENTS.md")
        );
        assert_eq!(
            t.target_path("~/.codex/AGENTS.md"),
            PathBuf::from("./.qoder/AGENTS.md")
        );
    }

    #[test]
    fn qoder_target_path_for_skill_file() {
        let t = qoder_target();
        assert_eq!(
            t.target_path(".claude/skills/my-skill/SKILL.md"),
            PathBuf::from("./.qoder/skills/my-skill/SKILL.md")
        );
    }

    #[test]
    fn qoder_target_path_for_agent_file() {
        let t = qoder_target();
        assert_eq!(
            t.target_path(".claude/agents/release-lead.md"),
            PathBuf::from("./.qoder/agents/release-lead.md")
        );
    }

    #[test]
    fn qoder_target_path_for_codex_sources() {
        let t = qoder_target();
        assert_eq!(
            t.target_path("~/.codex/skills/mtkf-coder/SKILL.md"),
            PathBuf::from("./.qoder/skills/mtkf-coder/SKILL.md")
        );
        assert_eq!(
            t.target_path("~/.codex/agents/mtkf-coder.toml"),
            PathBuf::from("./.qoder/agents/mtkf-coder.toml")
        );
        assert_eq!(
            t.target_path("~/.codex/config.toml ([mcp_servers])"),
            PathBuf::from("./.qoder/.mcp.json")
        );
        assert_eq!(
            t.target_path("~/.codex/memories/"),
            PathBuf::from("./.qoder/claude-memory-index/README.md")
        );
    }

    #[test]
    fn trae_target_paths() {
        let t = LayoutTarget::new(TargetLayout::trae(), "~/.trae", "./.trae");
        // instructions → AGENTS.md
        assert_eq!(
            t.target_path(".claude/CLAUDE.md"),
            PathBuf::from("./.trae/AGENTS.md")
        );
        // mcp → .trae/mcp.json
        assert_eq!(
            t.target_path("~/.codex/config.toml ([mcp_servers])"),
            PathBuf::from("./.trae/mcp.json")
        );
        // skills → .trae/skills/
        assert_eq!(
            t.target_path("~/.codex/skills/foo/SKILL.md"),
            PathBuf::from("./.trae/skills/foo/SKILL.md")
        );
    }

    #[test]
    fn lingma_target_paths() {
        let t = LayoutTarget::new(TargetLayout::lingma(), "~/.lingma", "./.lingma");
        // instructions → LINGMA.md（灵码特有）
        assert_eq!(
            t.target_path(".claude/CLAUDE.md"),
            PathBuf::from("./.lingma/LINGMA.md")
        );
        // mcp → .lingma/mcp-settings.json
        assert_eq!(
            t.target_path("~/.codex/config.toml ([mcp_servers])"),
            PathBuf::from("./.lingma/mcp-settings.json")
        );
        // agents → .lingma/agents/
        assert_eq!(
            t.target_path("~/.codex/agents/mtkf-coder.toml"),
            PathBuf::from("./.lingma/agents/mtkf-coder.toml")
        );
    }

    #[test]
    fn write_creates_file_with_content() {
        let dir = std::env::temp_dir().join(format!("migrator-tgt-{}", std::process::id()));
        let t = LayoutTarget::new(TargetLayout::qoder(), dir.join("home"), dir.join("proj"));
        let path = t.target_path(".claude/CLAUDE.md");
        let result = ConversionResult {
            content: "synced".into(),
            manual_review_required: false,
            manual_notes: vec![],
        };
        let wr = t.write(&path, &result).unwrap();
        assert!(wr.created);
        assert_eq!(fs::read_to_string(&path).unwrap(), "synced");
        fs::remove_dir_all(&dir).unwrap();
    }
}
