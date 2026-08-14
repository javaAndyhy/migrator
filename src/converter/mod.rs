//! 转换引擎 — 映射表驱动的配置转换
//!
//! 来源: migrate-to-codex 的映射表 (differences.md 四列结构)
//! 契约 v1: 映射表 schema 版本化，社区贡献基础
//! 语义降级: Partial 转换必须生成 MANUAL MIGRATION REQUIRED 块

pub mod converters;
pub mod manual_block;
pub mod mapping;

pub use mapping::{ConversionResult, MappingEntry, MappingTable, MappingStatus};
