//! ConfigSurface — 一个可迁移的配置单元
//!
//! 来源: migrate-to-codex 的 "surface" 概念
//! 每个配置面是一个独立的迁移单元，适配器按面扫描/转换。

use serde::{Deserialize, Serialize};

/// 配置面类型 — 迁移工具支持的所有配置单元
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SurfaceKind {
    /// 指令文件 (CLAUDE.md / AGENTS.md)
    Instructions,
    /// MCP 服务器配置 (.mcp.json / settings mcpServers)
    Mcp,
    /// Skills (.claude/skills)
    Skills,
    /// Agents / 子代理 (.claude/agents)
    Agents,
    /// Hooks (settings.json hooks)
    Hooks,
    /// Commands / slash 命令 (.claude/commands)
    Commands,
    /// 记忆索引 (memory)
    Memory,
}

impl SurfaceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SurfaceKind::Instructions => "instructions",
            SurfaceKind::Mcp => "mcp",
            SurfaceKind::Skills => "skills",
            SurfaceKind::Agents => "agents",
            SurfaceKind::Hooks => "hooks",
            SurfaceKind::Commands => "commands",
            SurfaceKind::Memory => "memory",
        }
    }
}

/// 配置面 — 扫描出的一个具体配置单元实例
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSurface {
    pub kind: SurfaceKind,
    /// 源侧相对路径（如 .claude/CLAUDE.md）
    pub source_path: String,
    /// 源侧是否实际存在
    pub present: bool,
    /// 备注（如格式差异、需人工审查等）
    pub notes: Option<String>,
}

impl ConfigSurface {
    pub fn new(kind: SurfaceKind, source_path: impl Into<String>, present: bool) -> Self {
        Self {
            kind,
            source_path: source_path.into(),
            present,
            notes: None,
        }
    }

    pub fn with_notes(mut self, notes: impl Into<String>) -> Self {
        self.notes = Some(notes.into());
        self
    }
}
