//! TargetLayout — 目标平台布局配置
//!
//! 平台间差异集中在"输出布局"，而非转换逻辑:
//!   - instructions 输出文件名 (AGENTS.md / ...)
//!   - MCP 输出路径 (.mcp.json / .trae/mcp.json / .lingma/mcp-settings.json)
//!   - 配置根目录 (.qoder / .trae / .lingma)
//!
//! 新增平台 = 定义一个新 Layout + 映射表 JSON
//!
//! 平台演进备忘（2026-08-18 官网核实）:
//!   - 通义灵码已更名 Qoder CN，LINGMA.md 弃用 → lingma 布局指令文件为项目根 AGENTS.md
//!   - Trae 品牌演进为 TraeWork（docs.trae.ai），配置路径待复核
//!
//! 平台演进备忘（2026-10-26 本机实测核实）:
//!   - DeepSeek Harness (dsh)：用户级 AGENTS.md = `$DSH_HOME/AGENTS.md`（默认 ~/.dsh/AGENTS.md）；
//!     skills 扫 `<project>/.dsh/skills` / `<project>/.agents/skills` / `~/.dsh/skills` / `~/.agents/skills`；
//!     MCP **不是** JSON 文件，而是 profile 的 cordis patch YAML 插件条目
//!     （`~/.dsh/profiles/<profile>/cordis.patch.yml`，插件 `@deepseek-ai/dsh-mcp-client`）；
//!     无文件级 subagent（subagent 由 profile/preset 配置驱动）。
//!   - 豆包桌面客户端：无任何文件级配置契约，MCP「自定义连接器」与技能均存在
//!     客户端内部（%LOCALAPPDATA%\Doubao\User Data\Default\Local Storage\leveldb），
//!     只能由用户在 UI 粘贴。故 doubao 布局输出"手动迁移包"（给人复制，不给程序读）。

use std::path::{Path, PathBuf};

/// MCP 配置的目标格式 —— 决定写入时用哪种合并算法
///
/// 契约: 由布局声明，**禁止**在写入路径上用文件名/扩展名猜格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpFormat {
    /// `{"mcpServers": {...}}` JSON（Claude / Qoder / Trae / 灵码 / WorkBuddy / 豆包手动包）
    Json,
    /// TOML `[mcp_servers.*]`（Codex config.toml）
    Toml,
    /// cordis patch YAML 插件条目列表（DeepSeek Harness）
    CordisYamlPluginList,
}

/// 目标平台布局
#[derive(Debug, Clone)]
pub struct TargetLayout {
    /// 平台名称（如 "qoder" / "trae" / "lingma"）
    pub name: &'static str,
    /// 项目级配置目录名（如 ".qoder" / ".trae" / ".lingma"）
    pub project_dir_name: &'static str,
    /// 用户级配置目录名（如 "~/.qoder"）
    pub user_dir_name: &'static str,
    /// instructions 输出文件名（如 "AGENTS.md"）
    pub instructions_file: &'static str,
    /// instructions 是否输出到项目根（而非配置根目录）
    ///
    /// 官方依据（2026-08-18 经 context7/官网核实）:
    ///   - Qoder: 规则兼容 AGENTS.md，官方要求复制到项目目录（docs.qoder.com/user-guide/rules）
    ///   - Qoder CN（原通义灵码）: 支持三级 AGENTS.md（用户级 ~/.qoder/AGENTS.md / 项目级 / 本地级）
    ///   - Trae: 规则支持项目根 AGENTS.md
    ///   - WorkBuddy: MEMORY.md 特殊（托管块引用），不适用
    pub instructions_at_project_root: bool,
    /// instructions 是否输出到用户级配置根（如 WorkBuddy 的 ~/.workbuddy/MEMORY.md）
    pub instructions_at_user_root: bool,
    /// MCP 输出路径（如 ".mcp.json" / "mcp.json" / "mcp-settings.json"）
    pub mcp_file: &'static str,
    /// MCP 是否写入用户级配置根（如 WorkBuddy 的 ~/.workbuddy/mcp.json）
    /// false = 写入项目配置根（qoder/trae/lingma）
    pub mcp_at_user_root: bool,
    /// MCP 是否写入项目根（如 Claude 的项目根 .mcp.json）
    pub mcp_at_project_root: bool,
    /// MCP 目标格式（JSON / TOML / cordis YAML 插件表）
    pub mcp_format: McpFormat,
    /// MCP 是否位于 `<user_dir>/<子目录>/<profile>/` 下（dsh 专用）
    ///
    /// dsh 的 MCP 是 profile 级配置：`~/.dsh/profiles/<desktop|web>/cordis.patch.yml`。
    /// 具体 profile 无法静态写死，运行时按已存在目录探测（见 `mcp_path`）。
    pub mcp_profile_subdir: Option<&'static str>,
    /// 手动迁移包目录（豆包专用）：目标平台无文件级契约时，
    /// 把转换结果落在项目内该目录，供用户复制粘贴到客户端 UI。
    pub manual_bundle_dir: Option<&'static str>,
    /// skills 子目录名（如 "skills"）
    pub skills_dir: &'static str,
    /// skills 是否输出到用户级配置根（如 WorkBuddy 的 ~/.workbuddy/skills/）
    pub skills_at_user_root: bool,
    /// agents 子目录名（如 "agents"）
    pub agents_dir: &'static str,
    /// agents 是否输出到用户级配置根（如 Codex 的 ~/.codex/agents/*.toml）
    pub agents_at_user_root: bool,
    /// memory 索引子目录名（如 "claude-memory-index"）
    pub memory_dir: &'static str,
    /// commands 子目录名（如 "commands"，Claude Code 的 .claude/commands/）
    pub commands_dir: &'static str,
    /// hooks 子目录名（如 "hooks"，Claude Code 的 .claude/hooks/）
    pub hooks_dir: &'static str,
}

impl TargetLayout {
    /// Qoder 布局（默认）
    ///
    /// 官方（docs.qoder.com/user-guide/rules）: AGENTS.md 放项目根即被识别。
    pub fn qoder() -> Self {
        Self {
            name: "qoder",
            project_dir_name: ".qoder",
            user_dir_name: "~/.qoder",
            instructions_file: "AGENTS.md",
            instructions_at_project_root: true,
            instructions_at_user_root: false,
            mcp_file: ".mcp.json",
            mcp_at_user_root: false,
            mcp_at_project_root: false,
            mcp_format: McpFormat::Json,
            mcp_profile_subdir: None,
            manual_bundle_dir: None,
            skills_dir: "skills",
            skills_at_user_root: false,
            agents_dir: "agents",
            agents_at_user_root: false,
            memory_dir: "claude-memory-index",
            commands_dir: "commands",
            hooks_dir: "hooks",
        }
    }

    /// Trae 布局（字节）
    /// 官方文档: MCP 配置在项目根 .trae/mcp.json; 规则支持项目根 AGENTS.md
    pub fn trae() -> Self {
        Self {
            name: "trae",
            project_dir_name: ".trae",
            user_dir_name: "~/.trae",
            instructions_file: "AGENTS.md",
            instructions_at_project_root: true,
            instructions_at_user_root: false,
            mcp_file: "mcp.json",
            mcp_at_user_root: false,
            mcp_at_project_root: false,
            mcp_format: McpFormat::Json,
            mcp_profile_subdir: None,
            manual_bundle_dir: None,
            skills_dir: "skills",
            skills_at_user_root: false,
            agents_dir: "agents",
            agents_at_user_root: false,
            memory_dir: "claude-memory-index",
            commands_dir: "commands",
            hooks_dir: "hooks",
        }
    }

    /// 通义灵码布局（阿里，已更名 Qoder CN）
    ///
    /// 2026-08-18 官网核实（help.aliyun.com/zh/lingma）:
    ///   - 产品已整合为「Qoder CN」系列（原通义灵码）
    ///   - 规则: .lingma/rules/ 目录（IDE）+ 三级 AGENTS.md（CLI 用户级 ~/.qoder/AGENTS.md / 项目级 / 本地级）
    ///   - agents: .lingma/agents/*.md（frontmatter: name/description/tools）
    ///   - MCP: .lingma/mcp-settings.json（partial，schema 待核）
    pub fn lingma() -> Self {
        Self {
            name: "lingma",
            project_dir_name: ".lingma",
            user_dir_name: "~/.lingma",
            instructions_file: "AGENTS.md",
            instructions_at_project_root: true,
            instructions_at_user_root: false,
            mcp_file: "mcp-settings.json",
            mcp_at_user_root: false,
            mcp_at_project_root: false,
            mcp_format: McpFormat::Json,
            mcp_profile_subdir: None,
            manual_bundle_dir: None,
            skills_dir: "skills",
            skills_at_user_root: false,
            agents_dir: "agents",
            agents_at_user_root: false,
            memory_dir: "claude-memory-index",
            commands_dir: "commands",
            hooks_dir: "hooks",
        }
    }

    /// WorkBuddy 布局（腾讯 AI 编程产品）
    ///
    /// WorkBuddy 用户级配置（官方路径）:
    ///   - mcp.json: MCP 配置（mcpServers 字段，Claude 兼容），用户级 ~/.workbuddy/mcp.json
    ///   - skills/: 用户级技能（标准 SKILL.md）→ 迁移写入用户级 ~/.workbuddy/skills/
    ///   - MEMORY.md: 用户级记忆（跨项目）→ 迁移写入用户级 ~/.workbuddy/MEMORY.md（托管块引用）
    ///   - 无文件级 subagent（Expert 是市场概念）→ agents 标记 Unsupported
    ///   - 无只读记忆索引消费方 → memory 标记 Unsupported（正文留在源侧）
    ///   - 项目级配置根: {project}/.workbuddy/（保留，供项目级数据）
    pub fn workbuddy() -> Self {
        Self {
            name: "workbuddy",
            project_dir_name: ".workbuddy",
            user_dir_name: "~/.workbuddy",
            instructions_file: "MEMORY.md",
            instructions_at_project_root: false,
            instructions_at_user_root: true,
            mcp_file: "mcp.json",
            mcp_at_user_root: true,
            mcp_at_project_root: false,
            mcp_format: McpFormat::Json,
            mcp_profile_subdir: None,
            manual_bundle_dir: None,
            skills_dir: "skills",
            skills_at_user_root: true,
            agents_dir: "agents",
            agents_at_user_root: false,
            memory_dir: "memory-index",
            commands_dir: "commands",
            hooks_dir: "hooks",
        }
    }

    /// DeepSeek Harness 布局（dsh，本机 2026-10-26 实测核实）
    ///
    /// 依据 `@deepseek-ai/dsh-*` 官方 README（安装于 `~/.dsh/profiles/node_modules`）:
    ///   - instructions: 用户级恒为 `$DSH_HOME/AGENTS.md`（默认 `~/.dsh/AGENTS.md`），
    ///     项目侧按 projectRoot→cwd 逐层读 `AGENTS.md` / `CLAUDE.md` + `*.local.md` 覆盖层。
    ///     迁移取**用户级**，与 Codex 的 `~/.codex/AGENTS.md` 全局规则语义对齐。
    ///   - mcp: `~/.dsh/profiles/<profile>/cordis.patch.yml` 的插件条目列表，
    ///     每个 server 一条 `name: '@deepseek-ai/dsh-mcp-client'`，**非** JSON 文件。
    ///   - skills: `~/.dsh/skills`（user-dsh，rank 400）+ 项目 `.dsh/skills`（rank 100）。
    ///   - agents: subagent 由 profile/preset 配置驱动，无文件级 `agents/*.md` → Unsupported。
    ///   - commands/hooks/memory: 无对应文件契约 → Unsupported。
    pub fn dsh() -> Self {
        Self {
            name: "dsh",
            project_dir_name: ".dsh",
            user_dir_name: "~/.dsh",
            instructions_file: "AGENTS.md",
            instructions_at_project_root: false,
            instructions_at_user_root: true,
            mcp_file: "cordis.patch.yml",
            mcp_at_user_root: true,
            mcp_at_project_root: false,
            mcp_format: McpFormat::CordisYamlPluginList,
            mcp_profile_subdir: Some("profiles"),
            manual_bundle_dir: None,
            skills_dir: "skills",
            skills_at_user_root: true,
            agents_dir: "agents",
            agents_at_user_root: true,
            memory_dir: "memory-index",
            commands_dir: "commands",
            hooks_dir: "hooks",
        }
    }

    /// 豆包布局（豆包桌面客户端，本机 2026-10-26 实测核实）
    ///
    /// 官方客户端**无文件级配置契约**：MCP「自定义连接器」、技能、规则全部存在客户端
    /// 内部（`%LOCALAPPDATA%\Doubao\User Data\Default\Local Storage\leveldb`），
    /// 只能由用户界面粘贴。因此本布局输出「手动迁移包」：
    ///   `{project}/.doubao/manual/connectors.json` + `instructions.md` + `README.md`
    /// 供人复制到客户端 UI，**不假装**目标程序会读取。
    pub fn doubao() -> Self {
        Self {
            name: "doubao",
            project_dir_name: ".doubao",
            user_dir_name: "~/.doubao",
            instructions_file: "instructions.md",
            instructions_at_project_root: false,
            instructions_at_user_root: false,
            mcp_file: "connectors.json",
            mcp_at_user_root: false,
            mcp_at_project_root: false,
            mcp_format: McpFormat::Json,
            mcp_profile_subdir: None,
            manual_bundle_dir: Some(".doubao/manual"),
            skills_dir: "skills",
            skills_at_user_root: false,
            agents_dir: "agents",
            agents_at_user_root: false,
            memory_dir: "memory-index",
            commands_dir: "commands",
            hooks_dir: "hooks",
        }
    }

    /// Claude Code 布局（反向目标：国产平台 → Claude）
    ///
    /// Claude 官方结构（docs.anthropic.com/claude-code）:
    ///   - CLAUDE.md: 项目根规则文件
    ///   - .mcp.json: 项目根 MCP 配置（mcpServers）
    ///   - .claude/skills/、.claude/agents/*.md、.claude/commands/、.claude/hooks/
    pub fn claude() -> Self {
        Self {
            name: "claude",
            project_dir_name: ".claude",
            user_dir_name: "~/.claude",
            instructions_file: "CLAUDE.md",
            instructions_at_project_root: true,
            instructions_at_user_root: false,
            mcp_file: ".mcp.json",
            mcp_at_user_root: false,
            mcp_at_project_root: true,
            mcp_format: McpFormat::Json,
            mcp_profile_subdir: None,
            manual_bundle_dir: None,
            skills_dir: "skills",
            skills_at_user_root: false,
            agents_dir: "agents",
            agents_at_user_root: false,
            memory_dir: "claude-memory-index",
            commands_dir: "commands",
            hooks_dir: "hooks",
        }
    }

    /// Codex 布局（反向目标：国产平台 → Codex）
    ///
    /// Codex 官方结构（developers.openai.com/codex）:
    ///   - ~/.codex/AGENTS.md: 用户级全局规则
    ///   - ~/.codex/config.toml: [mcp_servers] 段（TOML）
    ///   - ~/.codex/skills/、~/.codex/agents/*.toml
    pub fn codex() -> Self {
        Self {
            name: "codex",
            project_dir_name: ".codex",
            user_dir_name: "~/.codex",
            instructions_file: "AGENTS.md",
            instructions_at_project_root: false,
            instructions_at_user_root: true,
            mcp_file: "config.toml",
            mcp_at_user_root: true,
            mcp_at_project_root: false,
            mcp_format: McpFormat::Toml,
            mcp_profile_subdir: None,
            manual_bundle_dir: None,
            skills_dir: "skills",
            skills_at_user_root: true,
            agents_dir: "agents",
            agents_at_user_root: true,
            memory_dir: "claude-memory-index",
            commands_dir: "commands",
            hooks_dir: "hooks",
        }
    }

    /// 按名称选择布局
    pub fn by_name(name: &str) -> Option<Self> {
        match name {
            "qoder" => Some(Self::qoder()),
            "trae" => Some(Self::trae()),
            "lingma" => Some(Self::lingma()),
            "workbuddy" => Some(Self::workbuddy()),
            "dsh" | "deepseek-harness" => Some(Self::dsh()),
            "doubao" => Some(Self::doubao()),
            "claude" | "claude-code" => Some(Self::claude()),
            "codex" => Some(Self::codex()),
            _ => None,
        }
    }

    /// 是否为「手动迁移包」布局（豆包：无文件级契约，产物供人粘贴）
    pub fn is_manual_bundle(&self) -> bool {
        self.manual_bundle_dir.is_some()
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

    /// 解析 MCP 配置的**实际落盘路径**
    ///
    /// 与 `mcp_at_user_root` / `mcp_at_project_root` 的区别只在于 dsh 的 profile 层：
    /// dsh 的 MCP 不是单一全局文件，而是 `~/.dsh/profiles/<profile>/cordis.patch.yml`，
    /// profile 名（desktop / web）无法静态写死，因此运行时探测：
    ///   1. 有 `desktop` 且其中存在 patch 文件 → desktop
    ///   2. 否则取第一个含 patch 文件的 profile 目录
    ///   3. 都没有 → 退化到 `desktop`（dsh 桌面客户端默认 profile，首次写入时创建）
    pub fn mcp_path(&self, user_dir: &Path, project_dir: &Path, project_root: &Path) -> PathBuf {
        // 手动迁移包（豆包）: MCP 产物落项目内，供人粘贴
        if let Some(dir) = self.manual_bundle_dir {
            return project_root.join(dir).join(self.mcp_file);
        }
        if let Some(sub) = self.mcp_profile_subdir {
            let profiles_root = user_dir.join(sub);
            let profile = pick_dsh_profile(&profiles_root, self.mcp_file);
            return profiles_root.join(profile).join(self.mcp_file);
        }
        if self.mcp_at_user_root {
            user_dir.join(self.mcp_file)
        } else if self.mcp_at_project_root {
            project_root.join(self.mcp_file)
        } else {
            project_dir.join(self.mcp_file)
        }
    }
}

/// 选择 dsh profile 目录名（见 `TargetLayout::mcp_path`）
fn pick_dsh_profile(profiles_root: &Path, patch_file: &str) -> String {
    const DEFAULT_PROFILE: &str = "desktop";
    if !profiles_root.is_dir() {
        return DEFAULT_PROFILE.to_string();
    }
    let preferred = profiles_root.join(DEFAULT_PROFILE).join(patch_file);
    if preferred.is_file() {
        return DEFAULT_PROFILE.to_string();
    }
    let mut names: Vec<String> = std::fs::read_dir(profiles_root)
        .map(|it| {
            it.flatten()
                .filter(|e| e.path().is_dir() && e.path().join(patch_file).is_file())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
        .into_iter()
        .next()
        .unwrap_or_else(|| DEFAULT_PROFILE.to_string())
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
        assert!(TargetLayout::by_name("dsh").is_some());
        assert!(TargetLayout::by_name("doubao").is_some());
        assert!(TargetLayout::by_name("unknown").is_none());
    }

    #[test]
    fn workbuddy_layout() {
        let wb = TargetLayout::workbuddy();
        assert_eq!(wb.name, "workbuddy");
        // MCP 走用户级官方路径 ~/.workbuddy/mcp.json（非项目级 .mcp.json）
        assert_eq!(wb.mcp_file, "mcp.json");
        assert!(wb.mcp_at_user_root);
        assert_eq!(wb.skills_dir, "skills");
        assert_eq!(wb.instructions_file, "MEMORY.md");
    }

    #[test]
    fn lingma_uses_lingma_md() {
        // Qoder CN（原灵码）已统一为 AGENTS.md（项目根），不再使用 LINGMA.md
        assert_eq!(TargetLayout::lingma().instructions_file, "AGENTS.md");
        assert!(TargetLayout::lingma().instructions_at_project_root);
        // Qoder 官方要求 AGENTS.md 放项目根
        assert_eq!(TargetLayout::qoder().instructions_file, "AGENTS.md");
        assert!(TargetLayout::qoder().instructions_at_project_root);
        // WorkBuddy 走 MEMORY.md 托管块，不适用项目根
        assert!(!TargetLayout::workbuddy().instructions_at_project_root);
    }

    #[test]
    fn trae_mcp_path() {
        assert_eq!(TargetLayout::trae().mcp_file, "mcp.json");
    }

    /// dsh 布局契约（2026-10-26 本机实测）:
    /// 用户级 AGENTS.md + profile 的 cordis patch YAML，MCP 格式不是 JSON
    #[test]
    fn dsh_layout_contract() {
        let d = TargetLayout::dsh();
        assert_eq!(d.name, "dsh");
        assert_eq!(d.user_dir_name, "~/.dsh");
        // 用户级全局规则文件
        assert_eq!(d.instructions_file, "AGENTS.md");
        assert!(d.instructions_at_user_root);
        assert!(!d.instructions_at_project_root);
        // MCP = cordis patch YAML 插件表（不是 mcp.json）
        assert_eq!(d.mcp_format, McpFormat::CordisYamlPluginList);
        assert_eq!(d.mcp_profile_subdir, Some("profiles"));
        assert_eq!(d.mcp_file, "cordis.patch.yml");
        // skills 走用户级 ~/.dsh/skills
        assert!(d.skills_at_user_root);
        // 无文件级 subagent
        assert!(!d.is_manual_bundle());
    }

    /// 豆包布局契约: 无文件级配置 → 手动迁移包
    #[test]
    fn doubao_layout_is_manual_bundle() {
        let d = TargetLayout::doubao();
        assert_eq!(d.name, "doubao");
        assert!(d.is_manual_bundle());
        assert_eq!(d.manual_bundle_dir, Some(".doubao/manual"));
        // 不写任何用户级配置根
        assert!(!d.instructions_at_user_root);
        assert!(!d.mcp_at_user_root);
    }

    /// dsh profile 探测: patch 文件存在时按实际 profile 解析，不存在时退化 desktop
    #[test]
    fn dsh_mcp_path_picks_existing_profile() {
        let base = std::env::var("CARGO_TARGET_TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir())
            .join(format!("migrator-dshprof-{}", std::process::id()));
        let user = base.join(".dsh");
        let proj = base.join("proj");
        let layout = TargetLayout::dsh();

        // 无 profiles 目录 → desktop
        assert_eq!(
            layout.mcp_path(&user, &proj.join(".dsh"), &proj),
            user.join("profiles/desktop/cordis.patch.yml")
        );

        // 只有 web profile → 选 web
        std::fs::create_dir_all(user.join("profiles/web")).unwrap();
        std::fs::write(user.join("profiles/web/cordis.patch.yml"), "[]").unwrap();
        assert_eq!(
            layout.mcp_path(&user, &proj.join(".dsh"), &proj),
            user.join("profiles/web/cordis.patch.yml")
        );

        // desktop 出现后优先 desktop
        std::fs::create_dir_all(user.join("profiles/desktop")).unwrap();
        std::fs::write(user.join("profiles/desktop/cordis.patch.yml"), "[]").unwrap();
        assert_eq!(
            layout.mcp_path(&user, &proj.join(".dsh"), &proj),
            user.join("profiles/desktop/cordis.patch.yml")
        );
        let _ = std::fs::remove_dir_all(&base);
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
        let dsh_root = Path::new("C:/Users/me/.dsh");
        assert_eq!(
            TargetLayout::dsh().project_root(dsh_root),
            PathBuf::from("C:/Users/me/.dsh")
        );
    }
}
