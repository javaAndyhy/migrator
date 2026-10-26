//! 布局驱动目标适配器 — 写入目标平台配置
//!
//! 平台差异 = TargetLayout（instructions 文件名 / MCP 路径 / 配置根目录）
//! 新增平台 = 在 layout.rs 定义一个新 Layout，无需改转换逻辑

use super::{TargetAdapter, TargetWriteResult};
use crate::converter::mapping::ConversionResult;
use crate::model::SurfaceKind;
use crate::target::layout::{McpFormat, TargetLayout};
use std::path::{Path, PathBuf};

/// 去掉源平台路径前缀，返回相对路径（支持正向与反向迁移的源前缀）
fn strip_source_prefix(source_path: &str) -> &str {
    source_path
        .trim_start_matches(".claude/")
        .trim_start_matches("~/.codex/")
        .trim_start_matches("~/.claude/")
        .trim_start_matches(".qoder/")
        .trim_start_matches(".trae/")
        .trim_start_matches(".lingma/")
        .trim_start_matches(".workbuddy/")
        .trim_start_matches("~/.workbuddy/")
        // dsh（DeepSeek Harness）: 用户级 ~/.dsh/、项目级 .dsh/
        .trim_start_matches("~/.dsh/")
        .trim_start_matches(".dsh/")
        // 豆包手动迁移包: .doubao/manual/
        .trim_start_matches("~/.doubao/")
        .trim_start_matches(".doubao/manual/")
        .trim_start_matches(".doubao/")
}

/// 取 `<dir>/` 之后的子路径；找不到则原样返回
///
/// 用于 skills / agents / commands 的目录内相对路径提取。
/// 相比旧实现的 `strip_prefix`，这里容忍源侧前缀未被完全剥离的情况。
fn sub_path_after<'a>(rel: &'a str, dir: &str) -> &'a str {
    let pat = format!("{dir}/");
    match rel.find(pat.as_str()) {
        Some(idx) => &rel[idx + pat.len()..],
        None => rel,
    }
}

/// 取文件名主干（去扩展名）；失败时回退原字符串
fn file_stem_of(rel: &str) -> String {
    Path::new(rel)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| rel.to_string())
}

/// 通用布局驱动目标适配器
pub struct LayoutTarget {
    /// 目标平台布局
    layout: TargetLayout,
    /// 用户级配置目录（预留: 用户级迁移使用，当前仅项目级写入）
    #[allow(dead_code)]
    user_dir: PathBuf,
    /// 项目级配置目录（如 {project}/.qoder）
    project_dir: PathBuf,
    /// 项目根目录（--project 传入值，instructions_at_project_root 时使用）
    project_root_dir: PathBuf,
}

impl LayoutTarget {
    pub fn new(
        layout: TargetLayout,
        user_dir: impl Into<PathBuf>,
        project_dir: impl Into<PathBuf>,
        project_root_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            layout,
            user_dir: user_dir.into(),
            project_dir: project_dir.into(),
            project_root_dir: project_root_dir.into(),
        }
    }

    /// 从当前目录按布局探测目标位置
    pub fn detect(layout: TargetLayout, current_dir: &Path) -> Self {
        Self::new(
            layout.clone(),
            layout.user_root(),
            layout.project_root(current_dir),
            current_dir.to_path_buf(),
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
    pub fn qoder_new(
        user_dir: impl Into<PathBuf>,
        project_dir: impl Into<PathBuf>,
        project_root_dir: impl Into<PathBuf>,
    ) -> Self {
        Self::new(
            TargetLayout::qoder(),
            user_dir,
            project_dir,
            project_root_dir,
        )
    }

    pub fn qoder_detect(current_dir: &Path) -> Self {
        Self::detect(TargetLayout::qoder(), current_dir)
    }
}

impl TargetAdapter for LayoutTarget {
    fn name(&self) -> &'static str {
        self.layout.name
    }

    fn user_dir(&self) -> PathBuf {
        self.user_dir.clone()
    }

    fn mcp_format(&self) -> McpFormat {
        self.layout.mcp_format
    }

    fn is_manual_bundle(&self) -> bool {
        self.layout.is_manual_bundle()
    }

    fn manual_bundle_dir(&self) -> Option<PathBuf> {
        self.layout
            .manual_bundle_dir
            .map(|d| self.project_root_dir.join(d))
    }

    /// 按配置面类型分派目标路径（契约: 绝不从路径字符串猜类型）
    fn target_path(&self, kind: SurfaceKind, source_path: &str) -> PathBuf {
        let rel = strip_source_prefix(source_path);
        let root = &self.project_dir;

        // 手动迁移包（豆包）: 目标平台无文件级契约，产物落项目内供人复制粘贴
        if let Some(dir) = self.layout.manual_bundle_dir {
            let base = self.project_root_dir.join(dir);
            return match kind {
                SurfaceKind::Instructions => base.join(self.layout.instructions_file),
                SurfaceKind::Mcp => base.join(self.layout.mcp_file),
                SurfaceKind::Skills => base
                    .join(self.layout.skills_dir)
                    .join(sub_path_after(rel, "skills")),
                SurfaceKind::Commands => {
                    let name = file_stem_of(sub_path_after(rel, "commands"));
                    base.join(self.layout.skills_dir)
                        .join(format!("{name}/SKILL.md"))
                }
                SurfaceKind::Agents => {
                    let name = file_stem_of(sub_path_after(rel, "agents"));
                    base.join(self.layout.agents_dir).join(format!("{name}.md"))
                }
                SurfaceKind::Memory => base.join("memory.md"),
                SurfaceKind::Hooks => base.join("hooks.md"),
            };
        }

        match kind {
            SurfaceKind::Instructions => {
                // 指令文件 → 平台自己的文件名（AGENTS.md / CLAUDE.md / MEMORY.md）
                // 优先级: 用户级（workbuddy/codex/dsh）> 项目根（qoder/trae/lingma/claude）> 配置根
                if self.layout.instructions_at_user_root {
                    self.user_dir.join(self.layout.instructions_file)
                } else if self.layout.instructions_at_project_root {
                    self.project_root_dir.join(self.layout.instructions_file)
                } else {
                    root.join(self.layout.instructions_file)
                }
            }
            SurfaceKind::Mcp => {
                // MCP 配置 → 平台自己的 MCP 文件
                // 优先级: dsh profile（~/.dsh/profiles/<p>/cordis.patch.yml）
                //        > 用户级（workbuddy ~/.workbuddy/mcp.json / codex ~/.codex/config.toml）
                //        > 项目根（claude .mcp.json）> 配置根（qoder/trae/lingma）
                self.layout
                    .mcp_path(&self.user_dir, root, &self.project_root_dir)
            }
            SurfaceKind::Skills => {
                // 保留 skill 目录内的完整相对路径（<name>/SKILL.md）
                let rest = sub_path_after(rel, "skills");
                if self.layout.skills_at_user_root {
                    self.user_dir.join(self.layout.skills_dir).join(rest)
                } else {
                    root.join(self.layout.skills_dir).join(rest)
                }
            }
            SurfaceKind::Commands => {
                // Claude slash 命令 → 单文件 skill（输出 skills/<name>/SKILL.md）
                let rest = sub_path_after(rel, "commands");
                let name = file_stem_of(rest);
                let sub = format!("{name}/SKILL.md");
                if self.layout.skills_at_user_root {
                    self.user_dir.join(self.layout.skills_dir).join(sub)
                } else {
                    root.join(self.layout.skills_dir).join(sub)
                }
            }
            SurfaceKind::Agents => {
                // agents: Codex 用户级为 *.toml，其余平台统一 *.md + YAML frontmatter
                let rest = sub_path_after(rel, "agents");
                let name = file_stem_of(rest);
                if self.layout.agents_at_user_root {
                    self.user_dir
                        .join(self.layout.agents_dir)
                        .join(format!("{name}.toml"))
                } else {
                    root.join(self.layout.agents_dir).join(format!("{name}.md"))
                }
            }
            SurfaceKind::Memory => {
                // memory 只读索引 → <配置根>/<memory_dir>/README.md
                root.join(self.layout.memory_dir).join("README.md")
            }
            SurfaceKind::Hooks => {
                // hooks 无通用目标形态（多数平台 Unsupported），保守落在配置根同名相对路径
                root.join(rel)
            }
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
        // 项目根 "./" + 配置根 "./.qoder"（AGENTS.md 走项目根）
        LayoutTarget::new(TargetLayout::qoder(), "~/.qoder", "./.qoder", "./")
    }

    #[test]
    fn qoder_target_path_for_instructions() {
        let t = qoder_target();
        // Qoder 官方: AGENTS.md 放项目根（非 .qoder/ 子目录）
        assert_eq!(
            t.target_path(SurfaceKind::Instructions, ".claude/CLAUDE.md"),
            PathBuf::from("./AGENTS.md")
        );
        assert_eq!(
            t.target_path(SurfaceKind::Instructions, "~/.codex/AGENTS.md"),
            PathBuf::from("./AGENTS.md")
        );
    }

    #[test]
    fn qoder_target_path_for_skill_file() {
        let t = qoder_target();
        assert_eq!(
            t.target_path(SurfaceKind::Skills, ".claude/skills/my-skill/SKILL.md"),
            PathBuf::from("./.qoder/skills/my-skill/SKILL.md")
        );
        // Claude slash 命令 → skills/<name>/SKILL.md
        assert_eq!(
            t.target_path(SurfaceKind::Commands, ".claude/commands/review.md"),
            PathBuf::from("./.qoder/skills/review/SKILL.md")
        );
    }

    #[test]
    fn qoder_target_path_for_agent_file() {
        let t = qoder_target();
        assert_eq!(
            t.target_path(SurfaceKind::Agents, ".claude/agents/release-lead.md"),
            PathBuf::from("./.qoder/agents/release-lead.md")
        );
    }

    #[test]
    fn qoder_target_path_for_codex_sources() {
        let t = qoder_target();
        assert_eq!(
            t.target_path(SurfaceKind::Skills, "~/.codex/skills/mtkf-coder/SKILL.md"),
            PathBuf::from("./.qoder/skills/mtkf-coder/SKILL.md")
        );
        assert_eq!(
            t.target_path(SurfaceKind::Agents, "~/.codex/agents/mtkf-coder.toml"),
            PathBuf::from("./.qoder/agents/mtkf-coder.md")
        );
        assert_eq!(
            t.target_path(SurfaceKind::Agents, ".claude/agents/release-lead.md"),
            PathBuf::from("./.qoder/agents/release-lead.md")
        );
        assert_eq!(
            t.target_path(SurfaceKind::Mcp, "~/.codex/config.toml ([mcp_servers])"),
            PathBuf::from("./.qoder/.mcp.json")
        );
        assert_eq!(
            t.target_path(SurfaceKind::Memory, "~/.codex/memories/"),
            PathBuf::from("./.qoder/claude-memory-index/README.md")
        );
    }

    // ---- 回归: 旧实现用路径字符串猜类型导致的静默数据丢失 ----

    #[test]
    fn skill_named_with_mcp_is_not_routed_to_mcp_file() {
        // 旧缺陷: target_path 的 contains("mcp") 分支排在 skills 之前，
        // 名字含 "mcp" 的 skill 会被写成 .mcp.json → 技能永久丢失。
        let t = qoder_target();
        assert_eq!(
            t.target_path(
                SurfaceKind::Skills,
                "~/.codex/skills/mtkf-mcp-tool/SKILL.md"
            ),
            PathBuf::from("./.qoder/skills/mtkf-mcp-tool/SKILL.md")
        );
        assert_eq!(
            t.target_path(SurfaceKind::Skills, ".claude/skills/mcp/SKILL.md"),
            PathBuf::from("./.qoder/skills/mcp/SKILL.md")
        );
        // agent 同理
        assert_eq!(
            t.target_path(SurfaceKind::Agents, ".claude/agents/mcp-reviewer.md"),
            PathBuf::from("./.qoder/agents/mcp-reviewer.md")
        );
        // command 同理
        assert_eq!(
            t.target_path(SurfaceKind::Commands, ".claude/commands/mcp-check.md"),
            PathBuf::from("./.qoder/skills/mcp-check/SKILL.md")
        );
    }

    #[test]
    fn skill_named_with_memor_is_not_routed_to_memory_index() {
        // 旧缺陷: rel.contains("memor") 会把 memory-keeper 这类 skill 写成记忆索引 README。
        let t = qoder_target();
        assert_eq!(
            t.target_path(SurfaceKind::Skills, ".claude/skills/memory-keeper/SKILL.md"),
            PathBuf::from("./.qoder/skills/memory-keeper/SKILL.md")
        );
    }

    #[test]
    fn workbuddy_source_instructions_reach_codex_agents_md() {
        // 旧缺陷: 源为 ~/.workbuddy/MEMORY.md 时 contains("memor") 命中，
        // instructions 被写进 claude-memory-index/README.md，AGENTS.md 空白。
        let t = LayoutTarget::new(TargetLayout::codex(), "~/.codex", "./.codex", "./");
        assert_eq!(
            t.target_path(SurfaceKind::Instructions, "~/.workbuddy/MEMORY.md"),
            PathBuf::from("~/.codex/AGENTS.md")
        );
        // 同理反向到 claude 应落在项目根 CLAUDE.md
        let c = LayoutTarget::new(TargetLayout::claude(), "~/.claude", "./.claude", "./");
        assert_eq!(
            c.target_path(SurfaceKind::Instructions, "~/.workbuddy/MEMORY.md"),
            PathBuf::from("./CLAUDE.md")
        );
    }

    #[test]
    fn codex_target_paths_for_reverse_migration() {
        // 反向: 国产平台 → Codex（用户级 ~/.codex）
        let t = LayoutTarget::new(TargetLayout::codex(), "~/.codex", "./.codex", "./");
        assert_eq!(
            t.target_path(SurfaceKind::Mcp, ".qoder/.mcp.json"),
            PathBuf::from("~/.codex/config.toml")
        );
        assert_eq!(
            t.target_path(SurfaceKind::Skills, ".qoder/skills/foo/SKILL.md"),
            PathBuf::from("~/.codex/skills/foo/SKILL.md")
        );
        // Codex agents 走用户级 *.toml
        assert_eq!(
            t.target_path(SurfaceKind::Agents, ".qoder/agents/bar.md"),
            PathBuf::from("~/.codex/agents/bar.toml")
        );
    }

    #[test]
    fn trae_target_paths() {
        let t = LayoutTarget::new(TargetLayout::trae(), "~/.trae", "./.trae", "./");
        // instructions → 项目根 AGENTS.md
        assert_eq!(
            t.target_path(SurfaceKind::Instructions, ".claude/CLAUDE.md"),
            PathBuf::from("./AGENTS.md")
        );
        // mcp → .trae/mcp.json
        assert_eq!(
            t.target_path(SurfaceKind::Mcp, "~/.codex/config.toml ([mcp_servers])"),
            PathBuf::from("./.trae/mcp.json")
        );
        // skills → .trae/skills/
        assert_eq!(
            t.target_path(SurfaceKind::Skills, "~/.codex/skills/foo/SKILL.md"),
            PathBuf::from("./.trae/skills/foo/SKILL.md")
        );
    }

    #[test]
    fn lingma_target_paths() {
        let t = LayoutTarget::new(TargetLayout::lingma(), "~/.lingma", "./.lingma", "./");
        // instructions → 项目根 AGENTS.md（Qoder CN 已弃用 LINGMA.md，支持三级 AGENTS.md）
        assert_eq!(
            t.target_path(SurfaceKind::Instructions, ".claude/CLAUDE.md"),
            PathBuf::from("./AGENTS.md")
        );
        // mcp → .lingma/mcp-settings.json
        assert_eq!(
            t.target_path(SurfaceKind::Mcp, "~/.codex/config.toml ([mcp_servers])"),
            PathBuf::from("./.lingma/mcp-settings.json")
        );
        // agents → .lingma/agents/ (YAML md 格式，统一 .md)
        assert_eq!(
            t.target_path(SurfaceKind::Agents, "~/.codex/agents/mtkf-coder.toml"),
            PathBuf::from("./.lingma/agents/mtkf-coder.md")
        );
    }

    #[test]
    fn workbuddy_mcp_goes_to_user_root() {
        // WorkBuddy MCP 必须写入用户级官方路径 ~/.workbuddy/mcp.json（非项目级）
        let t = LayoutTarget::new(
            TargetLayout::workbuddy(),
            "~/.workbuddy",
            "./.workbuddy",
            "./",
        );
        assert_eq!(
            t.target_path(SurfaceKind::Mcp, "~/.codex/config.toml ([mcp_servers])"),
            PathBuf::from("~/.workbuddy/mcp.json")
        );
        assert_eq!(
            t.target_path(SurfaceKind::Mcp, ".claude/.mcp.json"),
            PathBuf::from("~/.workbuddy/mcp.json")
        );
        // skills 走用户级 ~/.workbuddy/skills/
        assert_eq!(
            t.target_path(SurfaceKind::Skills, "~/.codex/skills/foo/SKILL.md"),
            PathBuf::from("~/.workbuddy/skills/foo/SKILL.md")
        );
        // instructions（MEMORY.md 引用）走用户级 ~/.workbuddy/MEMORY.md
        assert_eq!(
            t.target_path(SurfaceKind::Instructions, "~/.codex/AGENTS.md"),
            PathBuf::from("~/.workbuddy/MEMORY.md")
        );
        // 其余配置面仍走项目级 .workbuddy/
        assert_eq!(
            t.target_path(SurfaceKind::Agents, "~/.codex/agents/mtkf-coder.toml"),
            PathBuf::from("./.workbuddy/agents/mtkf-coder.md")
        );
    }

    /// dsh（DeepSeek Harness）路径契约: 用户级 AGENTS.md + profile 的 cordis patch YAML
    #[test]
    fn dsh_target_paths() {
        let t = LayoutTarget::new(TargetLayout::dsh(), "~/.dsh", "./.dsh", "./");
        assert_eq!(
            t.target_path(SurfaceKind::Instructions, "~/.codex/AGENTS.md"),
            PathBuf::from("~/.dsh/AGENTS.md")
        );
        assert_eq!(
            t.target_path(SurfaceKind::Mcp, "~/.codex/config.toml ([mcp_servers])"),
            PathBuf::from("~/.dsh/profiles/desktop/cordis.patch.yml")
        );
        assert_eq!(
            t.target_path(SurfaceKind::Skills, "~/.codex/skills/foo/SKILL.md"),
            PathBuf::from("~/.dsh/skills/foo/SKILL.md")
        );
        assert_eq!(t.mcp_format(), McpFormat::CordisYamlPluginList);
        assert!(!t.is_manual_bundle());
    }

    /// 豆包路径契约: 全部落「手动迁移包」，不触碰任何用户级配置
    #[test]
    fn doubao_target_paths_are_manual_bundle() {
        let t = LayoutTarget::new(TargetLayout::doubao(), "~/.doubao", "./.doubao", "./");
        assert!(t.is_manual_bundle());
        assert_eq!(
            t.manual_bundle_dir(),
            Some(PathBuf::from("./.doubao/manual"))
        );
        assert_eq!(
            t.target_path(SurfaceKind::Instructions, ".claude/CLAUDE.md"),
            PathBuf::from("./.doubao/manual/instructions.md")
        );
        assert_eq!(
            t.target_path(SurfaceKind::Mcp, ".claude/.mcp.json"),
            PathBuf::from("./.doubao/manual/connectors.json")
        );
        assert_eq!(
            t.target_path(SurfaceKind::Skills, ".claude/skills/foo/SKILL.md"),
            PathBuf::from("./.doubao/manual/skills/foo/SKILL.md")
        );
        assert_eq!(
            t.target_path(SurfaceKind::Commands, ".claude/commands/review.md"),
            PathBuf::from("./.doubao/manual/skills/review/SKILL.md")
        );
        assert_eq!(
            t.target_path(SurfaceKind::Agents, ".claude/agents/release-lead.md"),
            PathBuf::from("./.doubao/manual/agents/release-lead.md")
        );
    }

    #[test]
    fn write_creates_file_with_content() {
        let base = std::env::var("CARGO_TARGET_TMPDIR")
            .or_else(|_| std::env::var("MIGRATOR_TARGET_DIR"))
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        let dir = base.join(format!("migrator-tgt-{}", std::process::id()));
        let t = LayoutTarget::new(
            TargetLayout::qoder(),
            dir.join("home"),
            dir.join("proj"),
            dir.join("proj"),
        );
        let path = t.target_path(SurfaceKind::Instructions, ".claude/CLAUDE.md");
        let result = ConversionResult {
            content: "synced".into(),
            manual_review_required: false,
            manual_notes: vec![],
        };
        match t.write(&path, &result) {
            Ok(wr) => {
                assert!(wr.created);
                assert_eq!(fs::read_to_string(&path).unwrap(), "synced");
            }
            Err(e)
                if e.raw_os_error() == Some(5)
                    || e.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                eprintln!("跳过 write_creates_file_with_content：当前环境禁止 fsync ({e})");
            }
            Err(e) => panic!("write 不应失败：{e}"),
        }
        fs::remove_dir_all(&dir).unwrap();
    }
}
