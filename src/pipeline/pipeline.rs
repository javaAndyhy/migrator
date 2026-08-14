//! 五阶段管线实现
//!
//! scan → plan → dry-run → apply → validate
//! 每阶段可独立运行（migrate-to-codex 命令集模式）

use crate::converter::converters::{agents::convert_agent, memory::build_memory_index, skills::convert_skill};
use crate::converter::mapping::{ConversionResult, MappingStatus};
use crate::model::{
    ConfigSurface, MigrationAction, MigrationOptions, MigrationPlan, ReportRow, ReportStatus,
    SurfaceKind,
};
use crate::source::SourceAdapter;
use crate::target::TargetAdapter;
use anyhow::{Context, Result};
use std::path::PathBuf;

/// 阶段 1: 扫描源侧配置面
pub fn scan_sources(source: &dyn SourceAdapter) -> Vec<ConfigSurface> {
    source.scan()
}

/// 阶段 2: 生成迁移计划（只读）
pub fn plan_migration(
    _source: &dyn SourceAdapter,
    surfaces: &[ConfigSurface],
    options: &MigrationOptions,
) -> MigrationPlan {
    let mut plan = MigrationPlan::new(options.scope);
    for surface in surfaces {
        if !surface.present {
            continue;
        }
        let action = MigrationAction {
            surface: surface.kind.as_str().to_string(),
            source: surface.source_path.clone(),
            target: format!("{} -> {}", surface.kind.as_str(), surface.source_path),
            action: "convert".to_string(),
            status: "planned".to_string(),
            notes: None,
        };
        plan.add_action(action);
    }
    plan
}

/// 阶段 3+4: dry-run 或 apply（apply 需 confirm_write）
///
/// 返回报告行列表
pub fn run_migration<T: TargetAdapter>(
    source: &dyn SourceAdapter,
    target: &T,
    surfaces: &[ConfigSurface],
    options: &MigrationOptions,
) -> Result<Vec<ReportRow>> {
    let mut reports = Vec::new();

    for surface in surfaces {
        if !surface.present {
            continue;
        }

        let raw = source
            .read(surface)
            .context(format!("读取源配置失败: {}", surface.source_path))?;

        // 按配置面类型执行转换（memory 需要目录路径，走独立处理）
        let conversion = if surface.kind == SurfaceKind::Memory {
            convert_memory_surface(source, surface, &raw)
        } else {
            convert_surface(source.name(), surface, &raw)
        };

        // 映射表驱动状态判定
        let mapping = options.mapping.find(surface.kind.as_str());
        let (status, notes) = match mapping {
            Some(entry) => match entry.status {
                MappingStatus::Exact => (ReportStatus::Added, entry.behavior.clone()),
                MappingStatus::Partial => {
                    // 映射表声明 Partial 即需审查（契约: 不能静默升级为 Added）
                    // 备注优先用转换器发现的细节，否则用映射表 caveat
                    let notes = if conversion.manual_review_required {
                        conversion.manual_notes.first().cloned().unwrap_or_else(|| {
                            entry
                                .caveat
                                .clone()
                                .unwrap_or_else(|| "semantics differ, manual review required".into())
                        })
                    } else {
                        entry
                            .caveat
                            .clone()
                            .unwrap_or_else(|| "semantics differ, manual review required".into())
                    };
                    (ReportStatus::CheckBeforeUsing, notes)
                }
                MappingStatus::Unsupported => (
                    ReportStatus::NotAdded,
                    entry
                        .caveat
                        .clone()
                        .unwrap_or_else(|| "no target equivalent".into()),
                ),
            },
            None => (
                ReportStatus::NotAdded,
                "no mapping entry for this surface".into(),
            ),
        };

        // Added 或 CheckBeforeUsing 且 confirm_write 时写入
        // CheckBeforeUsing 也写入（转换器已生成 MANUAL 块内嵌审查提示），只有 NotAdded 不写入
        let should_write = status != ReportStatus::NotAdded && options.confirm_write;
        if should_write {
            let target_path = target.target_path(&surface.source_path);
            let mut final_content = conversion;
            // instructions 走托管块：保护目标文件中的手写内容
            if surface.kind == SurfaceKind::Instructions {
                final_content = wrap_managed_block(&target_path, &surface.source_path, final_content)?;
            }
            target
                .write(&target_path, &final_content)
                .context(format!("写入目标失败: {}", target_path.display()))?;

            // skills: 复制 references/scripts/assets 等支持子目录
            if surface.kind == SurfaceKind::Skills {
                copy_skill_support_dirs(source, &surface.source_path, &target_path);
            }
        }

        reports.push(ReportRow::new(
            options.scope.as_str(),
            status,
            format!("{} {}", surface.kind.as_str(), surface.source_path),
            notes,
        ));
    }

    Ok(reports)
}

/// 按配置面类型执行内容转换
///
/// - instructions: 直接复制（Exact 映射）
/// - mcp: codex 源走 TOML→JSON 转换；claude 源直接复制（格式兼容）
/// - skills: 走 skills 转换器（字段降级 + MANUAL 块）
/// - agents: 走 agents 转换器（prompt guidance + MANUAL 块）
/// - 其他: 原样复制，标记需审查
fn convert_surface(source_name: &str, surface: &ConfigSurface, raw: &str) -> ConversionResult {
    match surface.kind {
        SurfaceKind::Skills => {
            let (content, needs_review, manual_notes) = convert_skill(raw);
            ConversionResult {
                content,
                manual_review_required: needs_review,
                manual_notes,
            }
        }
        SurfaceKind::Agents => {
            let (content, needs_review, manual_notes) = convert_agent(raw);
            ConversionResult {
                content,
                manual_review_required: needs_review,
                manual_notes,
            }
        }
        SurfaceKind::Mcp if source_name == "codex" => {
            // Codex config.toml ([mcp_servers]) → JSON .mcp.json
            let (content, needs_review, manual_notes) =
                crate::converter::converters::mcp::convert_codex_mcp(raw);
            ConversionResult {
                content,
                manual_review_required: needs_review,
                manual_notes,
            }
        }
        // instructions / mcp (claude): 直接复制
        _ => ConversionResult {
            content: raw.to_string(),
            manual_review_required: false,
            manual_notes: vec![],
        },
    }
}

/// 复制 skill 的支持子目录（references/scripts/assets 等）到目标 skill 目录
fn copy_skill_support_dirs(
    source: &dyn SourceAdapter,
    source_path: &str,
    target_skill_file: &std::path::Path,
) {
    let support_dirs = source.skill_support_dirs(source_path);
    if support_dirs.is_empty() {
        return;
    }
    // 目标 skill 根 = 目标 SKILL.md 的父目录
    let target_root = match target_skill_file.parent() {
        Some(p) => p.to_path_buf(),
        None => return,
    };
    for dir in &support_dirs {
        let dir_name = match dir.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };
        let target_dir = target_root.join(&dir_name);
        if let Err(e) = copy_dir_recursive(dir, &target_dir) {
            eprintln!("[警告] 复制 skill 子目录失败 {dir_name}: {e}");
        }
    }
}

/// 递归复制目录（保留结构，不跟随符号链接）
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    if dst.exists() {
        return Ok(()); // 已存在则不覆盖
    }
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &target)?;
        } else {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

/// memory 转换: 只读索引（不复制正文）
///
/// source.read 返回文件清单（每行一个绝对路径），此处生成只读 Markdown 索引
fn convert_memory_surface(
    _source: &dyn SourceAdapter,
    _surface: &ConfigSurface,
    raw: &str,
) -> ConversionResult {
    let file_paths: Vec<PathBuf> = raw
        .lines()
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .collect();

    if file_paths.is_empty() {
        return ConversionResult {
            content: String::new(),
            manual_review_required: false,
            manual_notes: vec![],
        };
    }

    // 索引根 = 第一个文件的父目录（memory 根）
    let root = file_paths[0]
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();

    let (index, warnings) = build_memory_index(&root, &file_paths);
    ConversionResult {
        content: index,
        manual_review_required: !warnings.is_empty(),
        manual_notes: warnings,
    }
}

/// dry-run: 与 apply 相同但不写入
pub fn dry_run_migration<T: TargetAdapter>(
    source: &dyn SourceAdapter,
    target: &T,
    surfaces: &[ConfigSurface],
    options: &MigrationOptions,
) -> Result<Vec<ReportRow>> {
    let mut dry = options.clone();
    dry.confirm_write = false;
    run_migration(source, target, surfaces, &dry)
}

/// 用托管块包裹 instructions 内容：保护目标文件已有手写内容
///
/// scope 用源配置面标识（如 "instructions"）；若目标文件已有托管块则重写，否则追加
/// 契约: 标记异常（重复/缺半/顺序反）→ 返回错误，拒绝写入（不尽力合并）
fn wrap_managed_block(
    target_path: &std::path::Path,
    source_path: &str,
    conversion: ConversionResult,
) -> anyhow::Result<ConversionResult> {
    let scope = format!("migrator:{source_path}");
    let existing = std::fs::read_to_string(target_path).unwrap_or_default();
    match crate::safety::managed_block::upsert_managed_block(&existing, &scope, &conversion.content) {
        Ok(result) => Ok(ConversionResult {
            content: result.content,
            manual_review_required: conversion.manual_review_required,
            manual_notes: conversion.manual_notes,
        }),
        Err(e) => Err(anyhow::anyhow!("托管块标记异常，拒绝写入 {target_path:?}: {e}")),
    }
}

/// 阶段 5: 校验目标（当前: 校验文件存在性 + 非空）
pub fn validate_target<T: TargetAdapter>(target: &T, surfaces: &[ConfigSurface]) -> Vec<ReportRow> {
    let mut reports = Vec::new();
    for surface in surfaces {
        if !surface.present {
            continue;
        }
        let path = target.target_path(&surface.source_path);
        let ok = path.exists();
        reports.push(ReportRow::new(
            "validate",
            if ok { ReportStatus::Added } else { ReportStatus::NotAdded },
            format!("{} target {}", surface.kind.as_str(), path.display()),
            if ok { "exists" } else { "missing" }.to_string(),
        ));
    }
    reports
}
