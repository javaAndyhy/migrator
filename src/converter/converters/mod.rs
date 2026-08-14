//! 转换器 — 各类配置面的内容转换逻辑
//!
//! 每个转换器: 输入源内容 → 输出目标内容（含 MANUAL 块）
//! 契约: Partial 转换必须生成 MANUAL 块

pub mod agents;
pub mod mcp;
pub mod memory;
pub mod skills;
