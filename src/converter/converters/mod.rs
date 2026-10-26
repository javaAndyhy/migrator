//! 转换器 — 各类配置面的内容转换逻辑
//!
//! 每个转换器: 输入源内容 → 输出目标内容（含 MANUAL 块）
//! 契约: Partial 转换必须生成 MANUAL 块

pub mod agents;
pub mod commands;
pub mod mcp;
/// dsh（DeepSeek Harness）的 MCP 是 cordis patch YAML 插件条目，非 JSON 文件
pub mod mcp_yaml;
pub mod memory;
pub mod skills;
