//! Qoder 目标适配器 — 写入 Qoder CLI 配置
//!
//! Qoder 与 Claude Code MCP 格式完全兼容（官方迁移工具验证）
//! 指令文件: CLAUDE.md → AGENTS.md（Qoder 读 AGENTS.md）

use super::{TargetAdapter, TargetWriteResult};
use crate::converter::mapping::ConversionResult;
use std::path::{Path, PathBuf};

/// 去掉源平台路径前缀（.claude/ 或 ~/.codex/），返回相对路径
fn strip_source_prefix(source_path: &str) -> &str {
    source_path
        .trim_start_matches(".claude/")
        .trim_start_matches("~/.codex/")
        .trim_start_matches("~/.claude/")
}

/// Qoder 目标适配器
pub struct QoderTarget {
    /// 用户级配置目录 (~/.qoder)
    user_dir: PathBuf,
    /// 项目级配置目录 (./.qoder)
    project_dir: PathBuf,
}

impl QoderTarget {
    pub fn new(user_dir: impl Into<PathBuf>, project_dir: impl Into<PathBuf>) -> Self {
        Self {
            user_dir: user_dir.into(),
            project_dir: project_dir.into(),
        }
    }

    /// 从当前目录探测 .qoder 位置
    pub fn detect(current_dir: &Path) -> Self {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".into());
        Self::new(
            PathBuf::from(home).join(".qoder"),
            current_dir.join(".qoder"),
        )
    }

    fn ensure_dir(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }
}

impl TargetAdapter for QoderTarget {
    fn name(&self) -> &'static str {
        "qoder"
    }

    fn target_path(&self, source_path: &str) -> PathBuf {
        // 去掉源平台路径前缀（.claude/ 或 ~/.codex/），得到相对路径
        let rel = strip_source_prefix(source_path);

        // 按配置类型推导目标路径
        if source_path.ends_with("CLAUDE.md") || source_path.ends_with("AGENTS.md") {
            // 指令文件统一输出到 AGENTS.md
            self.project_dir.join("AGENTS.md")
        } else if source_path.contains(".mcp.json")
            || source_path.contains("config.toml")
            || source_path.contains("mcp")
        {
            // MCP 配置统一输出到 .mcp.json
            self.project_dir.join(".mcp.json")
        } else if let Some(rest) = rel.strip_prefix("skills/") {
            // skills/<name>/SKILL.md -> .qoder/skills/<name>/SKILL.md
            self.project_dir.join("skills").join(rest)
        } else if let Some(rest) = rel.strip_prefix("agents/") {
            // agents/<name>.toml -> .qoder/agents/<name>.toml
            self.project_dir.join("agents").join(rest)
        } else if rel.contains("memor") {
            // memory/memories 索引输出到 .qoder/claude-memory-index/
            self.project_dir.join("claude-memory-index").join("README.md")
        } else {
            self.project_dir.join(rel)
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

    #[test]
    fn target_path_for_instructions() {
        let t = QoderTarget::new("~/.qoder", "./.qoder");
        assert_eq!(
            t.target_path(".claude/CLAUDE.md"),
            PathBuf::from("./.qoder/AGENTS.md")
        );
        // codex 源的 AGENTS.md 也统一输出到目标 AGENTS.md
        assert_eq!(
            t.target_path("~/.codex/AGENTS.md"),
            PathBuf::from("./.qoder/AGENTS.md")
        );
    }

    #[test]
    fn target_path_for_skill_file() {
        let t = QoderTarget::new("~/.qoder", "./.qoder");
        assert_eq!(
            t.target_path(".claude/skills/my-skill/SKILL.md"),
            PathBuf::from("./.qoder/skills/my-skill/SKILL.md")
        );
    }

    #[test]
    fn target_path_for_agent_file() {
        let t = QoderTarget::new("~/.qoder", "./.qoder");
        assert_eq!(
            t.target_path(".claude/agents/release-lead.md"),
            PathBuf::from("./.qoder/agents/release-lead.md")
        );
    }

    #[test]
    fn target_path_for_codex_sources() {
        let t = QoderTarget::new("~/.qoder", "./.qoder");
        // codex skills
        assert_eq!(
            t.target_path("~/.codex/skills/mtkf-coder/SKILL.md"),
            PathBuf::from("./.qoder/skills/mtkf-coder/SKILL.md")
        );
        // codex agents (toml)
        assert_eq!(
            t.target_path("~/.codex/agents/mtkf-coder.toml"),
            PathBuf::from("./.qoder/agents/mtkf-coder.toml")
        );
        // codex mcp (config.toml)
        assert_eq!(
            t.target_path("~/.codex/config.toml ([mcp_servers])"),
            PathBuf::from("./.qoder/.mcp.json")
        );
        // codex memory
        assert_eq!(
            t.target_path("~/.codex/memories/"),
            PathBuf::from("./.qoder/claude-memory-index/README.md")
        );
    }

    #[test]
    fn write_creates_file_with_content() {
        let dir = std::env::temp_dir().join(format!("migrator-tgt-{}", std::process::id()));
        let t = QoderTarget::new(dir.join("home"), dir.join("proj"));
        let path = t.target_path("instructions");
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

#[cfg(test)]
mod extra_tests {
    use super::*;
    use std::fs;

    #[test]
    fn write_different_dirs_no_interference() {
        let dir = std::env::temp_dir().join(format!("migrator-tgt2-{}", std::process::id()));
        let t = QoderTarget::new(dir.join("home"), dir.join("proj"));
        let path = t.target_path("instructions");
        let result = ConversionResult {
            content: "other".into(),
            manual_review_required: false,
            manual_notes: vec![],
        };
        t.write(&path, &result).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "other");
        fs::remove_dir_all(&dir).unwrap();
    }
}
