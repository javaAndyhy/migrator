//! TargetLayout — 目标平台布局配置
//!
//! 平台间差异集中在"输出布局"，而非转换逻辑:
//!   - instructions 输出文件名 (AGENTS.md / LINGMA.md / ...)
//!   - MCP 输出路径 (.mcp.json / .trae/mcp.json / .lingma/mcp-settings.json)
//!   - 配置根目录 (.qoder / .trae / .lingma)
//!
//! 新增平台 = 定义一个新 Layout + 映射表 JSON

use std::path::PathBuf;

/// 目标平台布局
#[derive(Debug, Clone)]
pub struct TargetLayout {
    /// 平台名称（如 "qoder" / "trae" / "lingma"）
    pub name: &'static str,
    /// 项目级配置目录名（如 ".qoder" / ".trae" / ".lingma"）
    pub project_dir_name: &'static str,
    /// 用户级配置目录名（如 "~/.qoder"）
    pub user_dir_name: &'static str,
    /// instructions 输出文件名（如 "AGENTS.md" / "LINGMA.md"）
    pub instructions_file: &'static str,
    /// MCP 输出路径（相对项目配置根，如 ".mcp.json" / "mcp.json" / "mcp-settings.json"）
    pub mcp_file: &'static str,
    /// skills 子目录名（如 "skills"）
    pub skills_dir: &'static str,
    /// agents 子目录名（如 "agents"）
    pub agents_dir: &'static str,
    /// memory 索引子目录名（如 "claude-memory-index"）
    pub memory_dir: &'static str,
}

impl TargetLayout {
    /// Qoder 布局（默认）
    pub fn qoder() -> Self {
        Self {
            name: "qoder",
            project_dir_name: ".qoder",
            user_dir_name: "~/.qoder",
            instructions_file: "AGENTS.md",
            mcp_file: ".mcp.json",
            skills_dir: "skills",
            agents_dir: "agents",
            memory_dir: "claude-memory-index",
        }
    }

    /// Trae 布局（字节）
    /// 官方文档: MCP 配置在项目根 .trae/mcp.json; 规则支持 AGENTS.md
    pub fn trae() -> Self {
        Self {
            name: "trae",
            project_dir_name: ".trae",
            user_dir_name: "~/.trae",
            instructions_file: "AGENTS.md",
            mcp_file: "mcp.json",
            skills_dir: "skills",
            agents_dir: "agents",
            memory_dir: "claude-memory-index",
        }
    }

    /// 通义灵码布局（阿里）
    /// 官方文档: 项目规则文件 LINGMA.md; MCP 配置 .lingma/mcp-settings.json; agents 在 .lingma/agents/
    pub fn lingma() -> Self {
        Self {
            name: "lingma",
            project_dir_name: ".lingma",
            user_dir_name: "~/.lingma",
            instructions_file: "LINGMA.md",
            mcp_file: "mcp-settings.json",
            skills_dir: "skills",
            agents_dir: "agents",
            memory_dir: "claude-memory-index",
        }
    }

    /// WorkBuddy 布局（腾讯 AI 编程产品）
    ///
    /// 实测 ~/.workbuddy 结构:
    ///   - .mcp.json: MCP 配置（mcpServers 字段，Claude 兼容 + type 扩展）
    ///   - skills/: 用户级技能（标准 SKILL.md）
    ///   - MEMORY.md: 用户级记忆（跨项目）
    ///   - 无文件级 subagent（Expert 是市场概念）→ agents 标记 Unsupported
    ///   - 项目级配置根: {project}/.workbuddy/
    pub fn workbuddy() -> Self {
        Self {
            name: "workbuddy",
            project_dir_name: ".workbuddy",
            user_dir_name: "~/.workbuddy",
            instructions_file: "MEMORY.md",
            mcp_file: ".mcp.json",
            skills_dir: "skills",
            agents_dir: "agents",
            memory_dir: "memory-index",
        }
    }

    /// 按名称选择布局
    pub fn by_name(name: &str) -> Option<Self> {
        match name {
            "qoder" => Some(Self::qoder()),
            "trae" => Some(Self::trae()),
            "lingma" => Some(Self::lingma()),
            "workbuddy" => Some(Self::workbuddy()),
            _ => None,
        }
    }

    /// 项目配置根目录
    ///
    /// 若 current_dir 本身就是配置根（如 --project 直接指向 ~/.qoder 的用户级迁移），
    /// 则不二次拼接配置目录名。
    pub fn project_root(&self, current_dir: &std::path::Path) -> PathBuf {
        if current_dir.file_name() == Some(std::ffi::OsStr::new(self.project_dir_name)) {
            current_dir.to_path_buf()
        } else {
            current_dir.join(self.project_dir_name)
        }
    }

    /// 用户配置根目录
    pub fn user_root(&self) -> PathBuf {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join(self.user_dir_name.trim_start_matches("~/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_by_name() {
        assert!(TargetLayout::by_name("qoder").is_some());
        assert!(TargetLayout::by_name("trae").is_some());
        assert!(TargetLayout::by_name("lingma").is_some());
        assert!(TargetLayout::by_name("workbuddy").is_some());
        assert!(TargetLayout::by_name("unknown").is_none());
    }

    #[test]
    fn workbuddy_layout() {
        let wb = TargetLayout::workbuddy();
        assert_eq!(wb.name, "workbuddy");
        assert_eq!(wb.mcp_file, ".mcp.json");
        assert_eq!(wb.skills_dir, "skills");
        assert_eq!(wb.instructions_file, "MEMORY.md");
    }

    #[test]
    fn lingma_uses_lingma_md() {
        assert_eq!(TargetLayout::lingma().instructions_file, "LINGMA.md");
    }

    #[test]
    fn trae_mcp_path() {
        assert_eq!(TargetLayout::trae().mcp_file, "mcp.json");
    }

    #[test]
    fn project_root_avoids_double_nesting() {
        use std::path::Path;
        // 项目级: --project 项目根 → 拼接配置目录
        let proj = Path::new("C:/work/myapp");
        assert_eq!(
            TargetLayout::qoder().project_root(proj),
            PathBuf::from("C:/work/myapp/.qoder")
        );
        // 用户级: --project 直接指向配置根 (~/.qoder) → 不二次拼接
        let config_root = Path::new("C:/Users/me/.qoder");
        assert_eq!(
            TargetLayout::qoder().project_root(config_root),
            PathBuf::from("C:/Users/me/.qoder")
        );
        // 其他平台同理
        let lingma_root = Path::new("C:/Users/me/.lingma");
        assert_eq!(
            TargetLayout::lingma().project_root(lingma_root),
            PathBuf::from("C:/Users/me/.lingma")
        );
    }
}
