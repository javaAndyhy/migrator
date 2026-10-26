use clap::{Parser, Subcommand};
use migrator::converter::mapping::MappingTable;
use migrator::model::{MigrationOptions, PlanScope, SurfaceKind};
use migrator::pipeline::{
    dry_run_migration, plan_migration, run_migration, scan_sources, validate_target,
    workbuddy_rules_path,
};
use migrator::source::{ClaudeCodeSource, CodexSource, LayoutSource, SourceAdapter};
use migrator::target::{LayoutTarget, TargetAdapter, TargetLayout};
use std::path::{Path, PathBuf};

/// migrator — 双向 Agent 配置迁移引擎（Claude/Codex ↔ 国产平台）
#[derive(Parser)]
#[command(name = "migrator", version, about)]
struct Cli {
    /// 映射表 JSON 文件路径 (默认内置 claude-to-qoder)
    #[arg(long, global = true)]
    mapping: Option<PathBuf>,
    /// 源平台: claude | codex | qoder | trae | lingma | workbuddy | dsh | doubao（默认 claude）
    #[arg(long, global = true, default_value = "claude")]
    source: String,
    /// 目标平台: qoder | trae | lingma(Qoder CN) | workbuddy | dsh(DeepSeek Harness) | doubao(豆包手动包) | claude | codex（默认 qoder）
    #[arg(long, global = true, default_value = "qoder")]
    target: String,
    /// 只处理指定配置面: instructions,mcp,skills,agents,commands,hooks,memory（逗号分隔，默认全部）
    ///
    /// 典型用法：`--only instructions` 只同步规则，不碰 MCP —— 避免写 MCP 时
    /// 客户端重载连接器、弹授权或刷新当前会话。
    #[arg(long, global = true, value_delimiter = ',')]
    only: Option<Vec<String>>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 扫描源侧配置面 (只读)
    Scan {
        /// 项目目录 (默认当前目录)
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// 生成迁移计划 (只读)
    Plan {
        /// 项目目录 (默认当前目录)
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// 干跑迁移 (只读，不写入)
    DryRun {
        /// 项目目录 (默认当前目录)
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// 执行迁移 (需 --yes 确认)
    Apply {
        /// 项目目录 (默认当前目录)
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// 确认写入 (必须显式提供)
        #[arg(long)]
        yes: bool,
    },
    /// 校验已迁移的目标
    Validate {
        /// 项目目录 (默认当前目录)
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// 列出备份批次 (只读)
    Backups {
        /// 项目目录 (默认当前目录)
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// 恢复备份 (默认演练；--yes 才执行回滚)
    Restore {
        /// 项目目录 (默认当前目录)
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// 指定批次 ID (默认最新)
        #[arg(long)]
        batch: Option<String>,
        /// 确认执行回滚
        #[arg(long)]
        yes: bool,
    },
    /// 清场：删除全部备份 (用户文件不受影响)
    Clean {
        /// 项目目录 (默认当前目录)
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// 确认清场
        #[arg(long)]
        yes: bool,
    },
}

/// 加载映射表（平台化，五层解析）
///
/// 解析顺序（优先级从高到低）:
///   1. `--mapping` 显式指定 → 加载该文件
///   2. `~/.migrator/mappings/<source>-to-<target>.json` → 用户级覆盖（与 CWD 无关）
///   3. `data/mappings/<source>-to-<target>.json` → 仓库内相对路径（源码/开发运行时）
///   4. 编译期内嵌 JSON → 单二进制自带全部 16 个组合
///   5. `MappingTable::builtin` 硬编码兜底（未知组合显式报错）
///
/// 契约（fail-fast）: 第 1/2/3 层**文件存在但加载失败即报错中止**，绝不静默降级。
/// 这三层的文件都是人主动提供的（命令行参数 / 主动放进覆盖目录 / 仓库内维护），
/// 静默换用另一张表等于悄悄改变迁移语义 —— 用户会以为自己的映射生效了。
///
/// 第 4 层内嵌表是本项目分发正确性的关键: 此前只有第 3 层的 CWD 相对路径，
/// 发布的单二进制在用户项目目录里找不到 `data/`，导致「加 JSON 零代码扩展」失效、
/// 逆向组合 JSON 成为死文件、同一命令在不同 CWD 下行为不一致。
fn load_mapping(
    mapping_path: Option<&std::path::Path>,
    source_name: &str,
    target_name: &str,
) -> anyhow::Result<MappingTable> {
    // CLI 源名 → 映射表源规范名（claude 规范名 claude-code，其余原样）
    let source_key = if source_name == "claude" || source_name == "claude-code" {
        "claude-code"
    } else {
        source_name
    };

    // 1. 显式 --mapping：用户点名要用这张表，加载失败必须报错，不得偷偷换表
    if let Some(path) = mapping_path {
        let table = MappingTable::load_from_json(path).map_err(|e| {
            anyhow::anyhow!(
                "指定的映射表加载失败，已中止: {}\n原因: {e}\n\
                 提示: --mapping 是显式意图，此时降级到其他映射表会静默改变迁移语义。\n\
                 请修正该文件后重试（schema 见 data/mappings/ 示例），或去掉 --mapping 使用默认映射。",
                path.display()
            )
        })?;
        println!(
            "[映射表] 已加载 (--mapping): {} → {}",
            table.source, table.target
        );
        return Ok(table);
    }

    // 2. 用户级覆盖目录：与 CWD 无关，发布后的二进制靠它实现零代码扩展
    if let Some(user_file) = MappingTable::user_mapping_file(source_name, target_name)
        && user_file.exists()
    {
        let table = MappingTable::load_from_json(&user_file).map_err(|e| {
            anyhow::anyhow!(
                "用户级映射表加载失败，已中止: {}\n原因: {e}\n\
                     提示: 该文件是你主动放入覆盖目录的，静默忽略会让你以为它已生效。\n\
                     请修正或删除该文件后重试。",
                user_file.display()
            )
        })?;
        println!(
            "[映射表] 已加载 (用户级 {}): {} → {}",
            user_file.display(),
            table.source,
            table.target
        );
        return Ok(table);
    }

    // 3. 仓库内相对路径（仅在项目根目录下运行时命中）
    let default_file = MappingTable::default_mapping_file(source_name, target_name);
    if default_file.exists() {
        let table = MappingTable::load_from_json(&default_file).map_err(|e| {
            anyhow::anyhow!(
                "仓库映射表加载失败，已中止: {}\n原因: {e}\n\
                 提示: 该文件是映射语义的真相源，损坏时降级到内置表会掩盖问题。",
                default_file.display()
            )
        })?;
        println!(
            "[映射表] 已加载 ({}): {} → {}",
            default_file.display(),
            table.source,
            table.target
        );
        return Ok(table);
    }

    // 4. 编译期内嵌表：保证任意 CWD / 任意安装方式下行为一致
    if let Some(table) = MappingTable::from_embedded(source_name, target_name) {
        println!(
            "[映射表] 已加载 (内嵌): {} → {}",
            table.source, table.target
        );
        return Ok(table);
    }

    // 5. 内置硬编码兜底（未知组合在此显式报错）
    MappingTable::builtin(source_key, target_name)
}

/// 选择源适配器: claude | codex | qoder | trae | lingma | workbuddy | dsh | doubao（双向）
///
/// 未知 --source 显式报错，不再静默回退到 ClaudeCodeSource（避免"以为迁 A→B 实际走默认"）。
fn build_source(source_name: &str, project: &Path) -> anyhow::Result<Box<dyn SourceAdapter>> {
    match source_name {
        "codex" => Ok(Box::new(CodexSource::detect())),
        "claude" | "claude-code" => Ok(Box::new(ClaudeCodeSource::detect(project))),
        // 反向迁移：国产平台作为源，用布局驱动读取
        "qoder" | "trae" | "lingma" | "workbuddy" | "dsh" | "deepseek-harness" | "doubao" => {
            let layout = TargetLayout::by_name(source_name).expect("known platform layout");
            Ok(Box::new(LayoutSource::detect(layout, project)))
        }
        _ => Err(anyhow::anyhow!(
            "未知 --source: {source_name}\n可选: claude | claude-code | codex | qoder | trae | lingma | workbuddy | dsh | doubao"
        )),
    }
}

/// 选择目标适配器: qoder | trae | lingma | workbuddy | dsh | doubao | claude | codex
fn build_target(target_name: &str, project: &Path) -> Result<LayoutTarget, anyhow::Error> {
    match TargetLayout::by_name(target_name) {
        Some(layout) => Ok(LayoutTarget::detect(layout, project)),
        None => Err(anyhow::anyhow!(
            "未知目标平台: {target_name} (可选: qoder | trae | lingma | workbuddy | dsh | doubao | claude | codex)"
        )),
    }
}

/// 按 `--only` 过滤配置面
///
/// 未知名称 fail-fast：静默忽略会让用户以为「只迁了 mcp」，实际什么都没迁。
fn filter_surfaces(
    surfaces: Vec<migrator::model::ConfigSurface>,
    only: Option<&[String]>,
) -> Result<Vec<migrator::model::ConfigSurface>, anyhow::Error> {
    let Some(list) = only else {
        return Ok(surfaces);
    };
    const VALID: [&str; 7] = [
        "instructions",
        "mcp",
        "skills",
        "agents",
        "commands",
        "hooks",
        "memory",
    ];
    for name in list {
        if !VALID.contains(&name.as_str()) {
            return Err(anyhow::anyhow!(
                "未知的配置面: {name}（可选: {}）",
                VALID.join(" / ")
            ));
        }
    }
    Ok(surfaces
        .into_iter()
        .filter(|s| list.iter().any(|n| n == s.kind.as_str()))
        .collect())
}

fn main() -> Result<(), anyhow::Error> {
    let cli = Cli::parse();
    let source_name = cli.source.clone();
    let target_name = cli.target.clone();
    let only = cli.only.clone();
    let mapping = load_mapping(cli.mapping.as_deref(), &source_name, &target_name)?;

    match cli.command {
        Commands::Scan { project } => {
            let source = build_source(&source_name, &project)?;
            let surfaces = scan_sources(&*source);
            println!("=== 扫描结果 (source: {}) ===", source.name());
            for s in &surfaces {
                let status = if s.present { "[✓]" } else { "[ ]" };
                println!("{status} {:10} {}", s.kind.as_str(), s.source_path);
            }
        }

        Commands::Plan { project } => {
            let source = build_source(&source_name, &project)?;
            let surfaces = filter_surfaces(scan_sources(&*source), only.as_deref())?;
            let options = MigrationOptions {
                scope: PlanScope::ProjectShared,
                confirm_write: false,
                mapping: mapping.clone(),
                only: only.clone(),
            };
            let plan = plan_migration(&*source, &surfaces, &options);
            println!("=== 迁移计划 (scope: {}) ===", plan.scope.as_str());
            for a in &plan.actions {
                println!("  {} {} -> {}", a.action, a.source, a.target);
            }
        }

        Commands::DryRun { project } => {
            let source = build_source(&source_name, &project)?;
            let surfaces = filter_surfaces(scan_sources(&*source), only.as_deref())?;
            let target = build_target(&target_name, &project)?;
            let options = MigrationOptions {
                scope: PlanScope::ProjectShared,
                confirm_write: false,
                mapping: mapping.clone(),
                only: only.clone(),
            };
            let reports = dry_run_migration(&*source, &target, &surfaces, &options)?;
            print_reports(&reports, "dry-run (未写入)");
        }

        Commands::Apply { project, yes } => {
            if !yes {
                println!("错误: 必须显式提供 --yes 才执行写入 (契约: 默认只读)");
                std::process::exit(1);
            }
            let source = build_source(&source_name, &project)?;
            let surfaces = filter_surfaces(scan_sources(&*source), only.as_deref())?;
            let target = build_target(&target_name, &project)?;
            let options = MigrationOptions {
                scope: PlanScope::ProjectShared,
                confirm_write: true,
                mapping: mapping.clone(),
                only: only.clone(),
            };
            // 写入前备份已存在的目标文件
            //
            // 契约（fail-closed）: 备份是唯一的回退通道。备份失败即代表"无退路"，
            // 此时继续写入等于在没有安全网的情况下覆盖用户真实配置
            // （如 ~/.codex/config.toml）。因此必须中止，不写任何目标文件。
            let backup_mgr = migrator::safety::backup::BackupManager::new(&project);
            let mut target_paths: Vec<std::path::PathBuf> = surfaces
                .iter()
                .filter(|s| s.present)
                .map(|s| target.target_path(s.kind, &s.source_path))
                .collect();
            // P1 #5: workbuddy instructions 还会额外写入 ~/.workbuddy/rules/<source>-rules.md，
            // 该文件不在 target.target_path 列表里，必须显式纳入备份，否则用户已有同名 rules 会被静默覆盖且无法回滚。
            if target.name() == "workbuddy" {
                for s in surfaces
                    .iter()
                    .filter(|s| s.present && s.kind == SurfaceKind::Instructions)
                {
                    target_paths.push(workbuddy_rules_path(&target, &s.source_path));
                }
            }
            match backup_mgr.create_batch(&target_paths) {
                Ok(Some(batch_id)) => println!("[备份] 已创建批次 {batch_id}"),
                Ok(None) => println!("[备份] 无已存在目标文件，跳过备份"),
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "备份失败，已中止 apply（未写入任何文件）: {e}\n\
                         提示: 备份是唯一回退通道，无备份时不允许覆盖既有配置。\n\
                         请先修复备份目录权限或磁盘空间后重试。"
                    ));
                }
            }
            let reports = run_migration(&*source, &target, &surfaces, &options)?;
            print_reports(&reports, "apply (已写入)");
        }

        Commands::Validate { project } => {
            let source = build_source(&source_name, &project)?;
            let surfaces = filter_surfaces(scan_sources(&*source), only.as_deref())?;
            let target = build_target(&target_name, &project)?;
            let reports = validate_target(&target, &surfaces);
            print_reports(&reports, "validate");
        }

        Commands::Backups { project } => {
            let backup_mgr = migrator::safety::backup::BackupManager::new(&project);
            match backup_mgr.list_batches() {
                Ok(batches) => {
                    println!("=== 备份批次 ({}) ===", batches.len());
                    for b in &batches {
                        let files = backup_mgr.plan_restore(Some(b)).unwrap_or_default();
                        println!("  {b} ({} 文件)", files.len());
                    }
                    if batches.is_empty() {
                        println!("  (无备份)");
                    }
                }
                Err(e) => eprintln!("错误: {e}"),
            }
        }

        Commands::Restore {
            project,
            batch,
            yes,
        } => {
            let backup_mgr = migrator::safety::backup::BackupManager::new(&project);
            if !yes {
                // 演练
                match backup_mgr.plan_restore(batch.as_deref()) {
                    Ok(plan) => {
                        println!("=== restore 演练 (未写入) ===");
                        for (orig, bak) in &plan {
                            println!("  将恢复: {orig} <- {bak}");
                        }
                        if plan.is_empty() {
                            println!("  (无备份)");
                        }
                        println!("提示: 加 --yes 执行回滚");
                    }
                    Err(e) => eprintln!("错误: {e}"),
                }
            } else {
                match backup_mgr.restore(batch.as_deref()) {
                    Ok(restored) => {
                        println!("=== restore (已回滚) ===");
                        for r in &restored {
                            println!("  已恢复: {r}");
                        }
                    }
                    Err(e) => eprintln!("错误: {e}"),
                }
            }
        }

        Commands::Clean { project, yes } => {
            if !yes {
                println!("错误: 必须显式提供 --yes 才执行清场 (契约: 默认只读)");
                std::process::exit(1);
            }
            let backup_mgr = migrator::safety::backup::BackupManager::new(&project);
            match backup_mgr.clean() {
                Ok(removed) => {
                    println!("=== clean (已清场) ===");
                    println!("  删除 {} 个备份批次，用户文件保留", removed.len());
                }
                Err(e) => eprintln!("错误: {e}"),
            }
        }
    }

    Ok(())
}

fn print_reports(reports: &[migrator::model::ReportRow], title: &str) {
    println!("=== {title} ===");
    if reports.is_empty() {
        println!("  (无)")
    }
    for r in reports {
        println!("  [{:20}] {} — {}", r.status.as_str(), r.item, r.notes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_source_unknown_reports_error() {
        // 未知 --source 必须显式报错，不得静默回退到 ClaudeCodeSource
        let r = build_source(
            "definitely-not-a-platform",
            &std::path::PathBuf::from("/nonexistent"),
        );
        assert!(r.is_err(), "未知 source 应报错而非静默兜底");
        let msg = r.err().unwrap().to_string();
        assert!(msg.contains("未知 --source"), "msg = {msg}");
    }

    #[test]
    fn build_source_known_is_ok() {
        // 已知 source 应成功构造（detect 为轻量构造，不写真实用户配置）
        let r = build_source("codex", &std::path::PathBuf::from("/nonexistent"));
        assert!(r.is_ok(), "已知 source codex 应成功构造");
    }

    fn surfaces() -> Vec<migrator::model::ConfigSurface> {
        use migrator::model::{ConfigSurface, SurfaceKind};
        vec![
            ConfigSurface::new(SurfaceKind::Instructions, "AGENTS.md", true),
            ConfigSurface::new(SurfaceKind::Mcp, "mcp.json", true),
            ConfigSurface::new(SurfaceKind::Skills, "s/SKILL.md", true),
        ]
    }

    #[test]
    fn filter_surfaces_none_keeps_all() {
        let out = filter_surfaces(surfaces(), None).unwrap();
        assert_eq!(out.len(), 3);
    }

    /// 回归: `--only instructions` 只能留下 instructions，
    /// 否则「只同步规则」会连带写 MCP，触发客户端重载连接器
    #[test]
    fn filter_surfaces_only_keeps_selected() {
        let only = vec!["instructions".to_string()];
        let out = filter_surfaces(surfaces(), Some(&only)).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind.as_str(), "instructions");
    }

    #[test]
    fn filter_surfaces_multiple_selected() {
        let only = vec!["instructions".to_string(), "mcp".to_string()];
        let out = filter_surfaces(surfaces(), Some(&only)).unwrap();
        assert_eq!(out.len(), 2);
    }

    /// 回归: 未知配置面必须 fail-fast。静默忽略会让用户以为迁了，实际一个没动。
    #[test]
    fn filter_surfaces_unknown_fails_fast() {
        let only = vec!["bogus".to_string()];
        let err = filter_surfaces(surfaces(), Some(&only)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("未知的配置面"), "msg = {msg}");
        assert!(msg.contains("instructions"), "应列出可选项: {msg}");
    }
}
