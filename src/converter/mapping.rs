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
            behavior: "copy".into(),
            caveat: None,
            status: MappingStatus::Exact,
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
        assert_eq!(table.find("hooks").unwrap().status, MappingStatus::Unsupported);
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
        assert!(matches!(err, MappingError::SchemaVersionMismatch { expected: 1, actual: 2 }));
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
}
