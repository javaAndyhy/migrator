//! 映射表 — 转换引擎的知识库（schema v1）
//!
//! 契约 v1 (keel §2 决策):
//!   - 映射表 schema 版本化
//!   - 每对 source×target 独立映射表
//!   - 社区贡献通过 JSON 数据文件，代码不写死映射
//!   - JSON 键: source 用 surface kind 名 (instructions/mcp/skills/agents/hooks/commands/memory)

use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

/// 映射表 schema 版本 — 契约冻结
pub const MAPPING_SCHEMA_VERSION: u32 = 1;

/// 映射状态 — 三态（对应 migrate-to-codex 的 Added/Check/Not Added）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MappingStatus {
    /// 直接映射，语义等价
    Exact,
    /// 部分映射，语义有差异，需人工审查
    Partial,
    /// 无映射，不支持
    Unsupported,
}

/// 单条映射 — 四列: source → target → behavior → caveat
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingEntry {
    /// 源配置项（surface kind 名或路径，如 "instructions"）
    pub source: String,
    /// 目标配置项（如 "AGENTS.md"）
    pub target: String,
    /// 迁移行为（如 "convert" / "copy" / "report-only"）
    pub behavior: String,
    /// 注意事项（如 "semantics differ"）
    pub caveat: Option<String>,
    /// 映射状态
    pub status: MappingStatus,
}

/// 映射表 — 一个 source×target 对的所有映射
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingTable {
    pub schema_version: u32,
    pub source: String,
    pub target: String,
    pub entries: Vec<MappingEntry>,
}

/// 转换结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionResult {
    /// 转换后的目标内容
    pub content: String,
    /// 是否需要人工审查（Partial 或 Unsupported 时）
    pub manual_review_required: bool,
    /// 审查备注
    pub manual_notes: Vec<String>,
}

impl MappingTable {
    pub fn new(source: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            schema_version: MAPPING_SCHEMA_VERSION,
            source: source.into(),
            target: target.into(),
            entries: Vec::new(),
        }
    }

    pub fn add_entry(&mut self, entry: MappingEntry) {
        self.entries.push(entry);
    }

    /// 查找源配置项的映射
    pub fn find(&self, source_key: &str) -> Option<&MappingEntry> {
        self.entries.iter().find(|e| e.source == source_key)
    }

    /// 从 JSON 文件加载映射表（校验 schema 版本）
    pub fn load_from_json(path: &Path) -> Result<Self, MappingError> {
        let json = std::fs::read_to_string(path)
            .map_err(|e| MappingError::Io(path.to_string_lossy().to_string(), e))?;
        Self::from_json_str(&json)
    }

    /// 从 JSON 字符串解析映射表（校验 schema 版本）
    pub fn from_json_str(json: &str) -> Result<Self, MappingError> {
        let table: MappingTable = serde_json::from_str(json)?;
        if table.schema_version != MAPPING_SCHEMA_VERSION {
            return Err(MappingError::SchemaVersionMismatch {
                expected: MAPPING_SCHEMA_VERSION,
                actual: table.schema_version,
            });
        }
        if table.entries.is_empty() {
            return Err(MappingError::EmptyEntries);
        }
        Ok(table)
    }

    /// 内置默认映射表（claude-code → qoder，v1）
    ///
    /// 当外部 JSON 文件不存在时使用。与 data/mappings/claude-to-qoder.json 保持一致。
    pub fn builtin_claude_to_qoder() -> Self {
        let mut table = Self::new("claude-code", "qoder");
        table.add_entry(MappingEntry {
            source: "instructions".into(),
            target: "AGENTS.md".into(),
            behavior: "convert".into(),
            caveat: None,
            status: MappingStatus::Exact,
        });
        table.add_entry(MappingEntry {
            source: "mcp".into(),
            target: ".mcp.json".into(),
            behavior: "merge".into(),
            caveat: Some("Claude .mcp.json compatible; merged preserving existing servers".into()),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "skills".into(),
            target: "skills".into(),
            behavior: "convert".into(),
            caveat: Some("skill frontmatter may need review".into()),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "agents".into(),
            target: "agents".into(),
            behavior: "convert".into(),
            caveat: Some("agent format compatibility unverified".into()),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "memory".into(),
            target: "claude-memory-index".into(),
            behavior: "index".into(),
            caveat: Some("read-only index generation, source content stays in Claude".into()),
            status: MappingStatus::Partial,
        });
        table
    }

    /// 内置映射表（claude-code → trae）
    ///
    /// 差异: Trae 无独立 agents 概念（用 AGENTS.md 承载）→ agents 标记 Unsupported
    pub fn builtin_claude_to_trae() -> Self {
        let mut table = Self::new("claude-code", "trae");
        table.add_entry(MappingEntry {
            source: "instructions".into(),
            target: "AGENTS.md".into(),
            behavior: "convert".into(),
            caveat: None,
            status: MappingStatus::Exact,
        });
        table.add_entry(MappingEntry {
            source: "mcp".into(),
            target: "mcp.json".into(),
            behavior: "merge".into(),
            caveat: Some("Claude .mcp.json compatible; merged preserving existing servers".into()),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "skills".into(),
            target: "skills".into(),
            behavior: "convert".into(),
            caveat: Some("skill frontmatter may need review".into()),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "agents".into(),
            target: "".into(),
            behavior: "report-only".into(),
            caveat: Some("Trae has no sub-agent concept; merge into AGENTS.md".into()),
            status: MappingStatus::Unsupported,
        });
        table.add_entry(MappingEntry {
            source: "memory".into(),
            target: "claude-memory-index".into(),
            behavior: "index".into(),
            caveat: Some("read-only index generation".into()),
            status: MappingStatus::Partial,
        });
        table
    }

    /// 内置映射表（claude-code → lingma，即 Qoder CN）
    ///
    /// 差异: 指令文件是项目根 AGENTS.md（LINGMA.md 已弃用）; mcp-settings.json 格式与 Claude .mcp.json 不完全一致
    pub fn builtin_claude_to_lingma() -> Self {
        let mut table = Self::new("claude-code", "lingma");
        table.add_entry(MappingEntry {
            source: "instructions".into(),
            target: "AGENTS.md".into(),
            behavior: "convert".into(),
            caveat: Some("Qoder CN (renamed from Lingma) reads project-root AGENTS.md; IDE rules live in .lingma/rules/".into()),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "mcp".into(),
            target: "mcp-settings.json".into(),
            behavior: "convert".into(),
            caveat: Some("Lingma mcp-settings.json schema may differ from Claude .mcp.json".into()),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "skills".into(),
            target: "skills".into(),
            behavior: "convert".into(),
            caveat: Some("skill frontmatter may need review".into()),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "agents".into(),
            target: "agents".into(),
            behavior: "convert".into(),
            caveat: Some("Lingma agents use YAML md format, may need review".into()),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "memory".into(),
            target: "claude-memory-index".into(),
            behavior: "index".into(),
            caveat: Some("read-only index generation".into()),
            status: MappingStatus::Partial,
        });
        table
    }

    /// 内置映射表（codex → qoder）
    ///
    /// 差异: Codex MCP 是 config.toml 的 [mcp_servers] 段 → 需 TOML→JSON 转换
    pub fn builtin_codex_to_qoder() -> Self {
        let mut table = Self::new("codex", "qoder");
        table.add_entry(MappingEntry {
            source: "instructions".into(),
            target: "AGENTS.md".into(),
            behavior: "convert".into(),
            caveat: None,
            status: MappingStatus::Exact,
        });
        table.add_entry(MappingEntry {
            source: "mcp".into(),
            target: ".mcp.json".into(),
            behavior: "merge".into(),
            caveat: Some(
                "TOML [mcp_servers] to JSON mcpServers; merged preserving existing servers".into(),
            ),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "skills".into(),
            target: "skills".into(),
            behavior: "convert".into(),
            caveat: Some("skill frontmatter may need review".into()),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "agents".into(),
            target: "agents".into(),
            behavior: "convert".into(),
            caveat: Some("Codex agents are TOML, sandbox semantics may differ".into()),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "memory".into(),
            target: "claude-memory-index".into(),
            behavior: "index".into(),
            caveat: Some("read-only index generation".into()),
            status: MappingStatus::Partial,
        });
        table
    }

    /// 内置映射表（codex → trae）
    ///
    /// 差异: Trae 无子 agent 概念 → agents Unsupported
    pub fn builtin_codex_to_trae() -> Self {
        let mut table = Self::new("codex", "trae");
        table.add_entry(MappingEntry {
            source: "instructions".into(),
            target: "AGENTS.md".into(),
            behavior: "convert".into(),
            caveat: None,
            status: MappingStatus::Exact,
        });
        table.add_entry(MappingEntry {
            source: "mcp".into(),
            target: "mcp.json".into(),
            behavior: "merge".into(),
            caveat: Some(
                "TOML [mcp_servers] to JSON mcpServers; merged preserving existing servers".into(),
            ),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "skills".into(),
            target: "skills".into(),
            behavior: "convert".into(),
            caveat: Some("skill frontmatter may need review".into()),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "agents".into(),
            target: "".into(),
            behavior: "report-only".into(),
            caveat: Some("Trae has no sub-agent concept; merge into AGENTS.md".into()),
            status: MappingStatus::Unsupported,
        });
        table.add_entry(MappingEntry {
            source: "memory".into(),
            target: "claude-memory-index".into(),
            behavior: "index".into(),
            caveat: Some("read-only index generation".into()),
            status: MappingStatus::Partial,
        });
        table
    }

    /// 内置映射表（codex → lingma）
    ///
    /// 差异: 指令文件 AGENTS.md（项目根）; mcp-settings.json 格式有差异
    pub fn builtin_codex_to_lingma() -> Self {
        let mut table = Self::new("codex", "lingma");
        table.add_entry(MappingEntry {
            source: "instructions".into(),
            target: "AGENTS.md".into(),
            behavior: "convert".into(),
            caveat: Some("Qoder CN (renamed from Lingma) reads project-root AGENTS.md; IDE rules live in .lingma/rules/".into()),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "mcp".into(),
            target: "mcp-settings.json".into(),
            behavior: "convert".into(),
            caveat: Some("TOML [mcp_servers] to JSON mcpServers; Lingma schema may differ".into()),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "skills".into(),
            target: "skills".into(),
            behavior: "convert".into(),
            caveat: Some("skill frontmatter may need review".into()),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "agents".into(),
            target: "agents".into(),
            behavior: "convert".into(),
            caveat: Some("Codex agents are TOML, Lingma agents use YAML md; needs review".into()),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "memory".into(),
            target: "claude-memory-index".into(),
            behavior: "index".into(),
            caveat: Some("read-only index generation".into()),
            status: MappingStatus::Partial,
        });
        table
    }

    /// 内置映射表（codex → workbuddy）
    ///
    /// WorkBuddy 用户级配置（官方路径）:
    ///   - mcp: 用户级 ~/.workbuddy/mcp.json（mcpServers 字段, Claude 兼容）— 合并时保留现有 server
    ///   - skills: skills/ (标准 SKILL.md) — 用户级技能
    ///   - instructions: 全文写入 ~/.workbuddy/rules/，MEMORY.md 仅保留托管块引用
    ///   - agents: 无文件级 subagent → Unsupported
    ///   - memory: 无只读记忆索引消费方 → Unsupported（正文留在源侧）
    pub fn builtin_codex_to_workbuddy() -> Self {
        let mut table = Self::new("codex", "workbuddy");
        table.add_entry(MappingEntry {
            source: "instructions".into(),
            target: "MEMORY.md".into(),
            behavior: "append-reference".into(),
            caveat: Some(
                "full rules copied to ~/.workbuddy/rules/; MEMORY.md keeps brief managed reference"
                    .into(),
            ),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "mcp".into(),
            target: "mcp.json".into(),
            behavior: "merge".into(),
            caveat: Some(
                "TOML->JSON; merged into ~/.workbuddy/mcp.json preserving existing servers".into(),
            ),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "skills".into(),
            target: "skills".into(),
            behavior: "convert".into(),
            caveat: Some("SKILL.md format compatible; frontmatter may need review".into()),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "agents".into(),
            target: "".into(),
            behavior: "report-only".into(),
            caveat: Some(
                "WorkBuddy has no file-based subagents (Expert is a marketplace concept)".into(),
            ),
            status: MappingStatus::Unsupported,
        });
        table.add_entry(MappingEntry {
            source: "memory".into(),
            target: "".into(),
            behavior: "report-only".into(),
            caveat: Some(
                "WorkBuddy has no read-only memory index consumer; memories stay on source side"
                    .into(),
            ),
            status: MappingStatus::Unsupported,
        });
        table
    }

    /// 内置映射表（claude → workbuddy）
    ///
    /// 与 codex → workbuddy 同布局；mcp 源为 Claude .mcp.json（JSON 兼容，直接 merge）。
    pub fn builtin_claude_to_workbuddy() -> Self {
        let mut table = Self::new("claude-code", "workbuddy");
        table.add_entry(MappingEntry {
            source: "instructions".into(),
            target: "MEMORY.md".into(),
            behavior: "append-reference".into(),
            caveat: Some(
                "full rules copied to ~/.workbuddy/rules/; MEMORY.md keeps brief managed reference"
                    .into(),
            ),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "mcp".into(),
            target: "mcp.json".into(),
            behavior: "merge".into(),
            caveat: Some("Claude .mcp.json format compatible; merged into ~/.workbuddy/mcp.json preserving existing servers".into()),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "skills".into(),
            target: "skills".into(),
            behavior: "convert".into(),
            caveat: Some("SKILL.md format compatible; frontmatter may need review".into()),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "agents".into(),
            target: "".into(),
            behavior: "report-only".into(),
            caveat: Some(
                "WorkBuddy has no file-based subagents (Expert is a marketplace concept)".into(),
            ),
            status: MappingStatus::Unsupported,
        });
        table.add_entry(MappingEntry {
            source: "memory".into(),
            target: "".into(),
            behavior: "report-only".into(),
            caveat: Some(
                "WorkBuddy has no read-only memory index consumer; memories stay on source side"
                    .into(),
            ),
            status: MappingStatus::Unsupported,
        });
        table
    }

    /// 国产平台 → claude/codex 的公共映射条目（反向迁移）
    fn domestic_common_entries(to_codex: bool) -> Vec<MappingEntry> {
        vec![
            MappingEntry {
                source: "instructions".into(),
                target: if to_codex { "AGENTS.md" } else { "CLAUDE.md" }.into(),
                behavior: "convert".into(),
                caveat: Some("rules copied verbatim (md-compatible)".into()),
                status: MappingStatus::Exact,
            },
            MappingEntry {
                source: "mcp".into(),
                target: if to_codex { "config.toml" } else { ".mcp.json" }.into(),
                behavior: if to_codex { "convert" } else { "merge" }.into(),
                caveat: Some(
                    if to_codex {
                        "JSON mcpServers -> [mcp_servers] TOML (approx, review)"
                    } else {
                        "JSON mcpServers compatible; merged preserving existing servers"
                    }
                    .into(),
                ),
                status: MappingStatus::Partial,
            },
            MappingEntry {
                source: "skills".into(),
                target: "skills".into(),
                behavior: "convert".into(),
                caveat: Some("SKILL.md format compatible; frontmatter may need review".into()),
                status: MappingStatus::Partial,
            },
            MappingEntry {
                source: "agents".into(),
                target: "agents".into(),
                behavior: "convert".into(),
                caveat: Some(
                    if to_codex {
                        "md frontmatter -> Codex TOML (approx, review)"
                    } else {
                        "md frontmatter compatible (Claude subagent)"
                    }
                    .into(),
                ),
                status: MappingStatus::Partial,
            },
            MappingEntry {
                source: "commands".into(),
                target: "".into(),
                behavior: "report-only".into(),
                caveat: Some("domestic platforms have no slash-command concept".into()),
                status: MappingStatus::Unsupported,
            },
            MappingEntry {
                source: "hooks".into(),
                target: "".into(),
                behavior: "report-only".into(),
                caveat: Some("domestic platforms have no hooks.json equivalent".into()),
                status: MappingStatus::Unsupported,
            },
            MappingEntry {
                source: "memory".into(),
                target: "".into(),
                behavior: "report-only".into(),
                caveat: Some("domestic platforms have no memory index consumer".into()),
                status: MappingStatus::Unsupported,
            },
        ]
    }

    /// 内置映射表（任意源 → dsh / DeepSeek Harness）
    ///
    /// dsh 的配置面与本项目此前的假设差异最大（2026-10-26 本机实测 + 官方包 README 核实）：
    ///   - instructions: 用户级 `~/.dsh/AGENTS.md`（dsh 先读它，再读项目侧 AGENTS.md 链）
    ///   - mcp: **不是 JSON 文件**，而是 profile 的 cordis patch YAML 插件条目
    ///     （`~/.dsh/profiles/<profile>/cordis.patch.yml`，插件 `@deepseek-ai/dsh-mcp-client`）
    ///   - skills: `~/.dsh/skills`（user-dsh 根）
    ///   - agents: subagent 由 preset/profile 驱动，无文件级 `agents/*.md` → Unsupported
    pub fn builtin_to_dsh(source: &str) -> Self {
        let mcp_caveat = if source == "codex" {
            "TOML [mcp_servers] -> cordis patch plugin entries (@deepseek-ai/dsh-mcp-client) in ~/.dsh/profiles/<profile>/cordis.patch.yml"
        } else if source == "claude-code" || source == "claude" {
            "JSON mcpServers -> cordis patch plugin entries (@deepseek-ai/dsh-mcp-client) in ~/.dsh/profiles/<profile>/cordis.patch.yml"
        } else {
            "source MCP config -> cordis patch plugin entries (@deepseek-ai/dsh-mcp-client) in ~/.dsh/profiles/<profile>/cordis.patch.yml"
        };
        let mut table = Self::new(source, "dsh");
        table.add_entry(MappingEntry {
            source: "instructions".into(),
            target: "AGENTS.md".into(),
            behavior: "convert".into(),
            caveat: Some(
                "written to user-level ~/.dsh/AGENTS.md (loaded before project AGENTS.md); managed block protects hand-written rules"
                    .into(),
            ),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "mcp".into(),
            target: "cordis.patch.yml".into(),
            behavior: "merge".into(),
            caveat: Some(mcp_caveat.into()),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "skills".into(),
            target: "skills".into(),
            behavior: "convert".into(),
            caveat: Some(
                "written to ~/.dsh/skills; dsh requires kebab-case name + description frontmatter"
                    .into(),
            ),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "agents".into(),
            target: "".into(),
            behavior: "report-only".into(),
            caveat: Some(
                "dsh subagents are preset/profile driven; no file-based agents/*.md".into(),
            ),
            status: MappingStatus::Unsupported,
        });
        table.add_entry(MappingEntry {
            source: "commands".into(),
            target: "skills".into(),
            behavior: "convert".into(),
            caveat: Some(
                "slash commands become one-file skills (dsh skills are user-invocable)".into(),
            ),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "hooks".into(),
            target: "".into(),
            behavior: "report-only".into(),
            caveat: Some("dsh has no hooks.json contract".into()),
            status: MappingStatus::Unsupported,
        });
        table.add_entry(MappingEntry {
            source: "memory".into(),
            target: "".into(),
            behavior: "report-only".into(),
            caveat: Some("dsh has no read-only memory index consumer".into()),
            status: MappingStatus::Unsupported,
        });
        table
    }

    /// 内置映射表（任意源 → 豆包 / Doubao desktop）
    ///
    /// 豆包桌面客户端**没有文件级配置契约**：MCP「自定义连接器」、技能、规则全部存在
    /// 客户端内部（`%LOCALAPPDATA%\Doubao\User Data\Default\Local Storage\leveldb`）。
    /// 因此所有可迁移面都是 `manual-paste`：产出手动包，由用户在客户端 UI 粘贴。
    pub fn builtin_to_doubao(source: &str) -> Self {
        let mut table = Self::new(source, "doubao");
        table.add_entry(MappingEntry {
            source: "instructions".into(),
            target: "instructions.md".into(),
            behavior: "manual-paste".into(),
            caveat: Some(
                "paste .doubao/manual/instructions.md into the client's custom instructions".into(),
            ),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "mcp".into(),
            target: "connectors.json".into(),
            behavior: "manual-paste".into(),
            caveat: Some(
                "MCP lives in the client leveldb store: add each server via 插件 -> + 添加 -> 新建自定义连接器"
                    .into(),
            ),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "skills".into(),
            target: "skills".into(),
            behavior: "manual-paste".into(),
            caveat: Some(
                "Doubao skills are marketplace-only; copied files serve as reference".into(),
            ),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "commands".into(),
            target: "skills".into(),
            behavior: "manual-paste".into(),
            caveat: Some("slash commands become one-file skills in the manual bundle".into()),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "agents".into(),
            target: "agents".into(),
            behavior: "manual-paste".into(),
            caveat: Some("no importable agent format; paste persona text manually".into()),
            status: MappingStatus::Partial,
        });
        table.add_entry(MappingEntry {
            source: "hooks".into(),
            target: "".into(),
            behavior: "report-only".into(),
            caveat: Some("Doubao has no hooks contract".into()),
            status: MappingStatus::Unsupported,
        });
        table.add_entry(MappingEntry {
            source: "memory".into(),
            target: "".into(),
            behavior: "report-only".into(),
            caveat: Some("Doubao has no memory index consumer".into()),
            status: MappingStatus::Unsupported,
        });
        table
    }

    /// 国产平台 → claude/codex 的内置映射（反向迁移）
    ///
    /// `file_agents=false` 的平台（dsh）没有文件级 subagent，agents 面降级为 Unsupported。
    fn builtin_from_domestic(source: &str, to_codex: bool, file_agents: bool) -> Self {
        let target = if to_codex { "codex" } else { "claude" };
        let mut table = Self::new(source, target);
        for mut entry in Self::domestic_common_entries(to_codex) {
            if !file_agents && entry.source == "agents" {
                entry.target = String::new();
                entry.behavior = "report-only".into();
                entry.caveat = Some("source platform has no file-based agents".into());
                entry.status = MappingStatus::Unsupported;
            }
            table.add_entry(entry);
        }
        table
    }

    /// 按 source×target 组合选择内置映射表（平台化）
    ///
    /// 未知组合**显式报错**（不再静默回退到 claude-code → qoder），
    /// 避免用户以为在迁移 A→B、实际走默认 B→qoder。
    pub fn builtin(source: &str, target: &str) -> anyhow::Result<Self> {
        match (source, target) {
            ("claude-code", "qoder") => Ok(Self::builtin_claude_to_qoder()),
            ("claude-code", "trae") => Ok(Self::builtin_claude_to_trae()),
            ("claude-code", "lingma") => Ok(Self::builtin_claude_to_lingma()),
            ("claude-code", "workbuddy") => Ok(Self::builtin_claude_to_workbuddy()),
            ("codex", "qoder") => Ok(Self::builtin_codex_to_qoder()),
            ("codex", "trae") => Ok(Self::builtin_codex_to_trae()),
            ("codex", "lingma") => Ok(Self::builtin_codex_to_lingma()),
            ("codex", "workbuddy") => Ok(Self::builtin_codex_to_workbuddy()),
            // 目标为 dsh / 豆包：任意源均可迁（含国产互转，如 qoder → dsh）
            (s, "dsh") => Ok(Self::builtin_to_dsh(s)),
            (s, "doubao") => Ok(Self::builtin_to_doubao(s)),
            // 反向迁移：国产平台 → claude/codex
            (s @ ("qoder" | "trae" | "lingma" | "workbuddy"), "claude") => {
                Ok(Self::builtin_from_domestic(s, false, true))
            }
            (s @ ("qoder" | "trae" | "lingma" | "workbuddy"), "codex") => {
                Ok(Self::builtin_from_domestic(s, true, true))
            }
            ("dsh" | "deepseek-harness", "claude") => {
                Ok(Self::builtin_from_domestic("dsh", false, false))
            }
            ("dsh" | "deepseek-harness", "codex") => {
                Ok(Self::builtin_from_domestic("dsh", true, false))
            }
            ("doubao", "claude") => Ok(Self::builtin_from_domestic("doubao", false, true)),
            ("doubao", "codex") => Ok(Self::builtin_from_domestic("doubao", true, true)),
            _ => Err(anyhow::anyhow!(
                "不支持的迁移组合: {source} → {target}\n\
                 内置映射覆盖: claude/codex → qoder/trae/lingma/workbuddy/dsh/doubao；\n\
                 以及 qoder/trae/lingma/workbuddy/dsh/doubao → claude/codex。\n\
                 其他组合请用 --mapping 指定自定义映射 JSON（schema 见 data/mappings/ 示例文件）。"
            )),
        }
    }

    /// 映射表数据文件路径: data/mappings/<source>-to-<target>.json
    ///
    /// 注意: 这是**相对当前工作目录**的路径，只在项目仓库根目录下运行时命中。
    /// 发布的单二进制在用户自己的项目目录运行时不会命中 —— 那种场景由
    /// `user_mapping_file`（用户级覆盖）与 `from_embedded`（编译期内嵌）兜住。
    ///
    /// source 用 CLI 名（claude/codex），target 用平台名（qoder/trae/lingma）。
    pub fn default_mapping_file(source: &str, target: &str) -> std::path::PathBuf {
        std::path::PathBuf::from("data")
            .join("mappings")
            .join(format!("{source}-to-{target}.json"))
    }

    /// 用户级映射表覆盖路径: ~/.migrator/mappings/<source>-to-<target>.json
    ///
    /// 契约: 发布后的二进制也必须支持「加一个 JSON 文件 = 扩展一个平台组合」。
    /// 仅依赖 CWD 相对的 `data/mappings/` 时，用户下载二进制后在自己项目目录运行
    /// 根本找不到该目录，想扩展组合被迫重新编译源码 —— 与 README 承诺不符。
    /// 本目录提供与 CWD 无关的稳定覆盖入口，优先级高于内嵌表。
    pub fn user_mapping_file(source: &str, target: &str) -> Option<std::path::PathBuf> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()?;
        if home.trim().is_empty() {
            return None;
        }
        Some(
            std::path::PathBuf::from(home)
                .join(".migrator")
                .join("mappings")
                .join(format!("{source}-to-{target}.json")),
        )
    }

    /// 编译期内嵌的映射表 JSON —— 单二进制自带全部 16 个组合
    ///
    /// 根因修复: `default_mapping_file` 是相对 CWD 的路径，而 CI Release 只分发单个
    /// 二进制、不带 `data/` 目录。用户在自己项目目录运行时找不到映射文件 → 静默降级
    /// 到 `builtin` 硬编码。后果有三：
    ///   1. README「新增平台组合 = 加一个 JSON，零代码改动」在发布后失效；
    ///   2. 逆向组合的 8 个 JSON 在发布场景成为死文件；
    ///   3. 同一条命令在不同 CWD 下解析到不同映射源，行为不可预测。
    ///
    /// 内嵌后二进制在任何目录下行为一致，且 JSON 成为映射语义的唯一真相源。
    ///
    /// key 用 CLI 名（claude/codex/qoder/...），与 `default_mapping_file` 文件名一致。
    pub fn embedded_mapping_json(source: &str, target: &str) -> Option<&'static str> {
        let key = format!("{source}-to-{target}");
        let json = match key.as_str() {
            // 正向: claude / codex → 国产平台
            "claude-to-qoder" => include_str!("../../data/mappings/claude-to-qoder.json"),
            "claude-to-trae" => include_str!("../../data/mappings/claude-to-trae.json"),
            "claude-to-lingma" => include_str!("../../data/mappings/claude-to-lingma.json"),
            "claude-to-workbuddy" => include_str!("../../data/mappings/claude-to-workbuddy.json"),
            "claude-to-dsh" => include_str!("../../data/mappings/claude-to-dsh.json"),
            "claude-to-doubao" => include_str!("../../data/mappings/claude-to-doubao.json"),
            "codex-to-qoder" => include_str!("../../data/mappings/codex-to-qoder.json"),
            "codex-to-trae" => include_str!("../../data/mappings/codex-to-trae.json"),
            "codex-to-lingma" => include_str!("../../data/mappings/codex-to-lingma.json"),
            "codex-to-workbuddy" => include_str!("../../data/mappings/codex-to-workbuddy.json"),
            "codex-to-dsh" => include_str!("../../data/mappings/codex-to-dsh.json"),
            "codex-to-doubao" => include_str!("../../data/mappings/codex-to-doubao.json"),
            // 逆向: 国产平台 → claude / codex
            "qoder-to-claude" => include_str!("../../data/mappings/qoder-to-claude.json"),
            "qoder-to-codex" => include_str!("../../data/mappings/qoder-to-codex.json"),
            "trae-to-claude" => include_str!("../../data/mappings/trae-to-claude.json"),
            "trae-to-codex" => include_str!("../../data/mappings/trae-to-codex.json"),
            "lingma-to-claude" => include_str!("../../data/mappings/lingma-to-claude.json"),
            "lingma-to-codex" => include_str!("../../data/mappings/lingma-to-codex.json"),
            "workbuddy-to-claude" => include_str!("../../data/mappings/workbuddy-to-claude.json"),
            "workbuddy-to-codex" => include_str!("../../data/mappings/workbuddy-to-codex.json"),
            "dsh-to-claude" => include_str!("../../data/mappings/dsh-to-claude.json"),
            "dsh-to-codex" => include_str!("../../data/mappings/dsh-to-codex.json"),
            "doubao-to-claude" => include_str!("../../data/mappings/doubao-to-claude.json"),
            "doubao-to-codex" => include_str!("../../data/mappings/doubao-to-codex.json"),
            _ => return None,
        };
        Some(json)
    }

    /// 从内嵌 JSON 解析映射表（source/target 用 CLI 名）
    ///
    /// 内嵌 JSON 在编译期就存在，解析失败说明仓库里的 JSON 本身损坏 —— 由
    /// `embedded_tables_all_parse` 回归测试在 CI 阶段拦截，运行时按 None 交给下一层。
    pub fn from_embedded(source: &str, target: &str) -> Option<Self> {
        let json = Self::embedded_mapping_json(source, target)?;
        Self::from_json_str(json).ok()
    }
}

/// 映射表错误
#[derive(Debug, Error)]
pub enum MappingError {
    #[error("映射表 schema 版本不兼容: 期望 v{expected}, 实际 v{actual}")]
    SchemaVersionMismatch { expected: u32, actual: u32 },
    #[error("映射表解析失败: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("映射表为空: 至少需要一条 entry")]
    EmptyEntries,
    #[error("读取映射表文件失败 ({0}): {1}")]
    Io(String, std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_table_finds_entry() {
        let mut table = MappingTable::new("claude", "qoder");
        table.add_entry(MappingEntry {
            source: ".claude/CLAUDE.md".into(),
            target: "AGENTS.md".into(),
            behavior: "convert".into(),
            caveat: None,
            status: MappingStatus::Exact,
        });
        let found = table.find(".claude/CLAUDE.md").unwrap();
        assert_eq!(found.target, "AGENTS.md");
        assert_eq!(found.status, MappingStatus::Exact);
    }

    #[test]
    fn missing_entry_returns_none() {
        let table = MappingTable::new("claude", "qoder");
        assert!(table.find(".claude/skills").is_none());
    }

    #[test]
    fn schema_version_is_v1() {
        let table = MappingTable::new("claude", "qoder");
        assert_eq!(table.schema_version, 1);
    }

    #[test]
    fn parse_json_table() {
        let json = r#"{
            "schema_version": 1,
            "source": "claude-code",
            "target": "qoder",
            "entries": [
                {"source": "instructions", "target": "AGENTS.md", "behavior": "convert", "status": "exact"}
            ]
        }"#;
        let table = MappingTable::from_json_str(json).unwrap();
        assert_eq!(table.source, "claude-code");
        assert_eq!(table.target, "qoder");
        let entry = table.find("instructions").unwrap();
        assert_eq!(entry.target, "AGENTS.md");
        assert_eq!(entry.status, MappingStatus::Exact);
    }

    #[test]
    fn parse_partial_and_unsupported() {
        let json = r#"{
            "schema_version": 1,
            "source": "claude-code",
            "target": "qoder",
            "entries": [
                {"source": "skills", "target": "skills", "behavior": "convert", "status": "partial", "caveat": "review"},
                {"source": "hooks", "target": "", "behavior": "report-only", "status": "unsupported"}
            ]
        }"#;
        let table = MappingTable::from_json_str(json).unwrap();
        assert_eq!(table.find("skills").unwrap().status, MappingStatus::Partial);
        assert_eq!(
            table.find("hooks").unwrap().status,
            MappingStatus::Unsupported
        );
    }

    #[test]
    fn reject_wrong_schema_version() {
        let json = r#"{
            "schema_version": 2,
            "source": "claude-code",
            "target": "qoder",
            "entries": [{"source": "instructions", "target": "AGENTS.md", "behavior": "convert", "status": "exact"}]
        }"#;
        let err = MappingTable::from_json_str(json).unwrap_err();
        assert!(matches!(
            err,
            MappingError::SchemaVersionMismatch {
                expected: 1,
                actual: 2
            }
        ));
    }

    #[test]
    fn reject_empty_entries() {
        let json = r#"{
            "schema_version": 1,
            "source": "claude-code",
            "target": "qoder",
            "entries": []
        }"#;
        let err = MappingTable::from_json_str(json).unwrap_err();
        assert!(matches!(err, MappingError::EmptyEntries));
    }

    #[test]
    fn builtin_table_has_5_entries() {
        let table = MappingTable::builtin_claude_to_qoder();
        assert_eq!(table.entries.len(), 5);
        assert!(table.find("instructions").is_some());
        assert!(table.find("mcp").is_some());
        assert!(table.find("skills").is_some());
        assert!(table.find("agents").is_some());
        assert!(table.find("memory").is_some());
    }

    #[test]
    fn load_from_json_file_roundtrip() {
        let dir = std::env::temp_dir().join(format!("migrator-map-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("mapping.json");
        let table = MappingTable::builtin_claude_to_qoder();
        std::fs::write(&path, serde_json::to_string_pretty(&table).unwrap()).unwrap();
        let loaded = MappingTable::load_from_json(&path).unwrap();
        assert_eq!(loaded.entries.len(), 5);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn builtin_selects_by_platform() {
        let trae = MappingTable::builtin("claude-code", "trae").unwrap();
        assert_eq!(trae.target, "trae");
        // Trae 无 agents → Unsupported
        assert_eq!(
            trae.find("agents").unwrap().status,
            MappingStatus::Unsupported
        );

        let lingma = MappingTable::builtin("claude-code", "lingma").unwrap();
        assert_eq!(lingma.target, "lingma");
        // Qoder CN（原灵码）指令文件为项目根 AGENTS.md（LINGMA.md 已弃用）
        assert_eq!(lingma.find("instructions").unwrap().target, "AGENTS.md");
        // 灵码 mcp 格式有差异 → Partial
        assert_eq!(lingma.find("mcp").unwrap().status, MappingStatus::Partial);

        let codex = MappingTable::builtin("codex", "qoder").unwrap();
        assert_eq!(codex.source, "codex");
        assert_eq!(codex.target, "qoder");
        assert_eq!(codex.find("mcp").unwrap().status, MappingStatus::Partial);
    }

    #[test]
    fn builtin_covers_all_codex_combos() {
        // codex→trae: agents unsupported
        let codex_trae = MappingTable::builtin("codex", "trae").unwrap();
        assert_eq!(codex_trae.target, "trae");
        assert_eq!(
            codex_trae.find("agents").unwrap().status,
            MappingStatus::Unsupported
        );
        assert_eq!(
            codex_trae.find("mcp").unwrap().status,
            MappingStatus::Partial
        );

        // codex→lingma: instructions 输出 AGENTS.md（Qoder CN 已弃用 LINGMA.md）
        let codex_lingma = MappingTable::builtin("codex", "lingma").unwrap();
        assert_eq!(codex_lingma.target, "lingma");
        assert_eq!(
            codex_lingma.find("instructions").unwrap().target,
            "AGENTS.md"
        );
        assert_eq!(
            codex_lingma.find("mcp").unwrap().status,
            MappingStatus::Partial
        );
    }

    #[test]
    fn builtin_codex_to_workbuddy() {
        let wb = MappingTable::builtin("codex", "workbuddy").unwrap();
        assert_eq!(wb.source, "codex");
        assert_eq!(wb.target, "workbuddy");
        // instructions → MEMORY.md（托管块仅保留引用，全文在 rules/）
        let instr = wb.find("instructions").unwrap();
        assert_eq!(instr.target, "MEMORY.md");
        assert_eq!(instr.behavior, "append-reference");
        // mcp → 用户级 mcp.json (merge)
        assert_eq!(wb.find("mcp").unwrap().target, "mcp.json");
        assert_eq!(wb.find("mcp").unwrap().behavior, "merge");
        // agents unsupported（WorkBuddy 无文件级 subagent）
        assert_eq!(
            wb.find("agents").unwrap().status,
            MappingStatus::Unsupported
        );
        // memory unsupported（WorkBuddy 无只读记忆索引消费方）
        assert_eq!(
            wb.find("memory").unwrap().status,
            MappingStatus::Unsupported
        );
        assert_eq!(wb.find("memory").unwrap().behavior, "report-only");
    }

    #[test]
    fn builtin_domestic_to_claude_and_codex() {
        // qoder → claude：instructions 输出 CLAUDE.md，mcp merge，agents md→md
        let qc = MappingTable::builtin("qoder", "claude").unwrap();
        assert_eq!(qc.target, "claude");
        assert_eq!(qc.find("instructions").unwrap().target, "CLAUDE.md");
        assert_eq!(qc.find("mcp").unwrap().behavior, "merge");
        // qoder → codex：instructions 输出 AGENTS.md，mcp JSON→TOML
        let qx = MappingTable::builtin("qoder", "codex").unwrap();
        assert_eq!(qx.target, "codex");
        assert_eq!(qx.find("instructions").unwrap().target, "AGENTS.md");
        assert_eq!(qx.find("mcp").unwrap().behavior, "convert");
        assert!(
            qx.find("mcp")
                .unwrap()
                .caveat
                .as_deref()
                .unwrap()
                .contains("TOML")
        );
        // workbuddy → claude 同样覆盖
        let wc = MappingTable::builtin("workbuddy", "claude").unwrap();
        assert_eq!(wc.find("instructions").unwrap().target, "CLAUDE.md");
    }

    #[test]
    fn builtin_unknown_combo_errors() {
        // 未定义组合（如 claude→cursor）应显式报错，而非静默回退 claude→qoder
        let unknown = MappingTable::builtin("claude-code", "cursor");
        assert!(unknown.is_err(), "未知组合必须报错，不得静默兜底");
        let err = unknown.unwrap_err().to_string();
        assert!(err.contains("不支持的迁移组合"), "err = {err}");
    }

    /// dsh 目标契约（2026-10-26 实测）: MCP 走 cordis patch、agents/memory unsupported
    #[test]
    fn builtin_to_dsh_contract() {
        for src in ["claude-code", "codex", "qoder", "workbuddy"] {
            let t = MappingTable::builtin(src, "dsh").unwrap();
            assert_eq!(t.target, "dsh");
            assert_eq!(t.find("instructions").unwrap().target, "AGENTS.md");
            assert_eq!(t.find("mcp").unwrap().target, "cordis.patch.yml");
            assert_eq!(t.find("mcp").unwrap().behavior, "merge");
            assert_eq!(
                t.find("agents").unwrap().status,
                MappingStatus::Unsupported,
                "{src}→dsh: dsh 无文件级 subagent"
            );
            assert_eq!(t.find("memory").unwrap().status, MappingStatus::Unsupported);
            // 7 个面齐备（claude/codex 侧无 commands 时也不缺）
            assert_eq!(t.entries.len(), 7, "{src}→dsh 应有 7 个配置面");
        }
    }

    /// 豆包目标契约: 全部 manual-paste（客户端无文件级配置）
    #[test]
    fn builtin_to_doubao_contract() {
        for src in ["claude-code", "codex", "qoder"] {
            let t = MappingTable::builtin(src, "doubao").unwrap();
            assert_eq!(t.target, "doubao");
            let instr = t.find("instructions").unwrap();
            assert_eq!(instr.behavior, "manual-paste");
            assert_eq!(instr.target, "instructions.md");
            let mcp = t.find("mcp").unwrap();
            assert_eq!(mcp.behavior, "manual-paste");
            assert_eq!(mcp.target, "connectors.json");
            assert_eq!(t.find("hooks").unwrap().status, MappingStatus::Unsupported);
        }
    }

    /// 反向: dsh 无文件级 agents → Unsupported；豆包手动包可读回
    #[test]
    fn builtin_from_dsh_and_doubao() {
        let dc = MappingTable::builtin("dsh", "claude").unwrap();
        assert_eq!(dc.find("instructions").unwrap().target, "CLAUDE.md");
        assert_eq!(
            dc.find("agents").unwrap().status,
            MappingStatus::Unsupported,
            "dsh 无文件级 agents"
        );
        let dx = MappingTable::builtin("dsh", "codex").unwrap();
        assert_eq!(dx.find("mcp").unwrap().target, "config.toml");

        let bc = MappingTable::builtin("doubao", "claude").unwrap();
        assert_eq!(bc.find("instructions").unwrap().target, "CLAUDE.md");
        assert_eq!(
            bc.find("agents").unwrap().status,
            MappingStatus::Partial,
            "豆包手动包含 agents/*.md"
        );
    }

    #[test]
    fn default_mapping_file_path() {
        let p = MappingTable::default_mapping_file("claude", "trae");
        assert_eq!(
            p,
            std::path::PathBuf::from("data/mappings/claude-to-trae.json")
        );
        let p2 = MappingTable::default_mapping_file("codex", "lingma");
        assert_eq!(
            p2,
            std::path::PathBuf::from("data/mappings/codex-to-lingma.json")
        );
    }

    /// #8 回归：逆向组合必须有独立 JSON 映射文件，且能被加载（优先于 builtin 硬编码）
    #[test]
    fn reverse_mapping_loaded_from_json() {
        for (src, tgt) in [
            ("qoder", "claude"),
            ("qoder", "codex"),
            ("trae", "claude"),
            ("trae", "codex"),
            ("lingma", "claude"),
            ("lingma", "codex"),
            ("workbuddy", "claude"),
            ("workbuddy", "codex"),
            ("dsh", "claude"),
            ("dsh", "codex"),
            ("doubao", "claude"),
            ("doubao", "codex"),
        ] {
            let f = MappingTable::default_mapping_file(src, tgt);
            assert!(f.exists(), "逆向映射 JSON 必须存在: {}", f.display());
            let t = MappingTable::load_from_json(&f)
                .unwrap_or_else(|e| panic!("{} 加载失败: {e}", f.display()));
            assert_eq!(t.source, src, "{} source 字段应为 {src}", f.display());
            assert_eq!(t.target, tgt, "{} target 字段应为 {tgt}", f.display());
            assert_eq!(t.entries.len(), 7, "{} 应含 7 个面", f.display());
            // commands/hooks/memory 此前是 builtin 硬编码里的"死代码"，现在由 JSON 承载
            assert!(
                t.find("commands").is_some(),
                "{} 缺 commands 条目",
                f.display()
            );
            assert!(t.find("hooks").is_some(), "{} 缺 hooks 条目", f.display());
            assert!(t.find("memory").is_some(), "{} 缺 memory 条目", f.display());
        }
    }

    /// 全部受支持的迁移组合（CLI 名），内嵌表与解析层测试共用
    const ALL_COMBOS: [(&str, &str); 24] = [
        ("claude", "qoder"),
        ("claude", "trae"),
        ("claude", "lingma"),
        ("claude", "workbuddy"),
        ("claude", "dsh"),
        ("claude", "doubao"),
        ("codex", "qoder"),
        ("codex", "trae"),
        ("codex", "lingma"),
        ("codex", "workbuddy"),
        ("codex", "dsh"),
        ("codex", "doubao"),
        ("qoder", "claude"),
        ("qoder", "codex"),
        ("trae", "claude"),
        ("trae", "codex"),
        ("lingma", "claude"),
        ("lingma", "codex"),
        ("workbuddy", "claude"),
        ("workbuddy", "codex"),
        ("dsh", "claude"),
        ("dsh", "codex"),
        ("doubao", "claude"),
        ("doubao", "codex"),
    ];

    /// #12 回归：16 个组合必须全部有内嵌表且可解析
    ///
    /// 这是分发正确性的守卫 —— 内嵌表是发布二进制在任意 CWD 下的映射来源。
    /// 若仓库里某个 JSON 损坏或被删，此测试在 CI 阶段即失败，而不是等用户
    /// 下载二进制后静默降级到硬编码 builtin。
    #[test]
    fn embedded_tables_all_parse() {
        for (src, tgt) in ALL_COMBOS {
            let json = MappingTable::embedded_mapping_json(src, tgt)
                .unwrap_or_else(|| panic!("缺内嵌映射表: {src} → {tgt}"));
            assert!(!json.trim().is_empty(), "{src}-to-{tgt} 内嵌 JSON 为空");
            let t = MappingTable::from_embedded(src, tgt)
                .unwrap_or_else(|| panic!("内嵌映射表解析失败: {src} → {tgt}"));
            assert_eq!(t.target, tgt, "{src}-to-{tgt} target 字段不符");
            assert!(!t.entries.is_empty(), "{src}-to-{tgt} 条目为空");
        }
    }

    /// #12 回归：内嵌表必须与仓库 data/mappings/ 下的文件逐字节一致
    ///
    /// include_str! 在编译期快照文件内容。若有人改了 JSON 却没重新编译（或改错了
    /// 路径导致内嵌到旧文件），运行时同一组合会因解析层不同而拿到不同映射语义。
    #[test]
    fn embedded_matches_repo_files() {
        for (src, tgt) in ALL_COMBOS {
            let f = MappingTable::default_mapping_file(src, tgt);
            let on_disk = std::fs::read_to_string(&f)
                .unwrap_or_else(|e| panic!("{} 读取失败: {e}", f.display()));
            let embedded = MappingTable::embedded_mapping_json(src, tgt)
                .unwrap_or_else(|| panic!("缺内嵌映射表: {src} → {tgt}"));
            assert_eq!(
                embedded,
                on_disk,
                "{} 与内嵌快照不一致（改了 JSON 需重新编译）",
                f.display()
            );
        }
    }

    /// #12 回归：内嵌表与硬编码 builtin 的映射语义必须一致（双份维护漂移守卫）
    ///
    /// 两者是同一语义的两份表述: 内嵌 JSON 服务运行时，builtin 是最终兜底。
    /// 若漂移，同一组合会因命中不同解析层而产生不同迁移结果 —— 属于最难排查的
    /// 一类缺陷。这里比较**语义关键**字段：behavior（决定转换动作）、status（迁移
    /// 信心等级）。不比较 `target`（纯展示标签，内嵌 JSON 与 builtin 文案口径本就
    /// 不同，例如 memory 内嵌标 ""、builtin 标 "claude-memory-index"，但二者都由
    /// SurfaceKind + layout 决定真实落盘路径，无行为差异），也不比较 caveat 文案。
    ///
    /// 守卫方向（只校验危险的单向）:
    ///   **builtin ⊆ embedded** —— builtin 承诺的每一个面，内嵌表都必须存在且
    ///   behavior/status 一致。这是危险方向：若 builtin 说 instructions=exact 而
    ///   内嵌说 partial，命中不同解析层会产生不同迁移结果。
    ///   反向（embedded ⊆ builtin）**不**校验：内嵌 JSON 是 scan 全量规格，天然包含
    ///   hooks(report-only) / commands(skill 子情形) 这类 builtin 故意省略的只读面，
    ///   不构成行为漂移。
    #[test]
    fn embedded_and_builtin_agree_on_semantics() {
        for (src, tgt) in ALL_COMBOS {
            let source_key = if src == "claude" { "claude-code" } else { src };
            let embedded = MappingTable::from_embedded(src, tgt)
                .unwrap_or_else(|| panic!("缺内嵌映射表: {src} → {tgt}"));
            let builtin = MappingTable::builtin(source_key, tgt)
                .unwrap_or_else(|e| panic!("builtin({source_key}, {tgt}) 失败: {e}"));

            // builtin ⊆ embedded：builtin 的每个面必须在内嵌表中一致存在
            for be in &builtin.entries {
                let ee = embedded.find(&be.source).unwrap_or_else(|| {
                    panic!("{src}→{tgt}: 内嵌表缺 builtin 声明的面 `{}`", be.source)
                });
                assert_eq!(
                    ee.behavior, be.behavior,
                    "{src}→{tgt}: 面 `{}` behavior 漂移（内嵌 {} vs builtin {}）",
                    be.source, ee.behavior, be.behavior
                );
                assert_eq!(
                    ee.status, be.status,
                    "{src}→{tgt}: 面 `{}` status 漂移（内嵌 {:?} vs builtin {:?}）",
                    be.source, ee.status, be.status
                );
            }
        }
    }

    /// #12 回归：用户级覆盖路径必须与当前工作目录无关
    ///
    /// 这是修复的核心诉求 —— 发布的二进制在任何目录下都要能找到用户放置的映射表。
    #[test]
    fn user_mapping_file_is_cwd_independent() {
        let p = MappingTable::user_mapping_file("qoder", "codex").expect("HOME/USERPROFILE 应可用");
        assert!(
            p.is_absolute(),
            "用户级映射路径必须是绝对路径: {}",
            p.display()
        );
        assert!(
            p.ends_with(std::path::Path::new(
                ".migrator/mappings/qoder-to-codex.json"
            )) || p
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with(".migrator/mappings/qoder-to-codex.json"),
            "路径结构不符: {}",
            p.display()
        );
    }
}
