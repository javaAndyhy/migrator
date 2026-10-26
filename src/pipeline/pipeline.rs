//! 五阶段管线实现
//!
//! scan → plan → dry-run → apply → validate
//! 每阶段可独立运行（migrate-to-codex 命令集模式）

use crate::converter::converters::{agents::convert_agent, memory::build_memory_index, skills::convert_skill};
use crate::converter::mapping::{ConversionResult, MappingStatus};
use crate::converter::converters::mcp_yaml;
use crate::model::{
    ConfigSurface, MigrationAction, MigrationOptions, MigrationPlan, ReportRow, ReportStatus,
    SurfaceKind,
};
use crate::source::SourceAdapter;
use crate::target::{McpFormat, TargetAdapter};
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
        // `--only` 过滤（双保险：main 已过滤 surfaces，这里再挡一次，
        // 保证直接调用 run_migration 的代码也受约束，不会意外写入未选中的面）
        if !options.wants(surface.kind.as_str()) {
            continue;
        }

        let raw = source
            .read(surface)
            .context(format!("读取源配置失败: {}", surface.source_path))?;

        // 按配置面类型执行转换（memory 需要目录路径，走独立处理）
        let conversion = if surface.kind == SurfaceKind::Memory {
            convert_memory_surface(source, surface, &raw)
        } else {
            convert_surface(source.name(), target.name(), surface, &raw)
        };

        // 映射表驱动状态判定
        let mapping = options.mapping.find(surface.kind.as_str());
        let (mut status, mut notes) = match mapping {
            Some(entry) => match entry.status {
                MappingStatus::Exact => (ReportStatus::Added, entry.behavior.clone()),
                MappingStatus::Partial => {
                    // 映射表声明 Partial 即需审查（契约: 不能静默升级为 Added）
                    // 备注优先用转换器发现的细节，否则用映射表 caveat
                    // 注意：转换器可能报多条（如 MCP 面逐个 server 的跳过原因），
                    // 旧实现只取 first() → 9 个 server 全被跳过时只显示 1 条，
                    // 用户会误以为「只有 1 个有问题」，实际 0 个迁成功。
                    let fallback = || {
                        entry
                            .caveat
                            .clone()
                            .unwrap_or_else(|| "semantics differ, manual review required".into())
                    };
                    let notes = if conversion.manual_review_required {
                        if conversion.manual_notes.is_empty() {
                            fallback()
                        } else {
                            summarize_notes(&conversion.manual_notes)
                        }
                    } else {
                        fallback()
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

        // MCP 面守卫：转换产物一个 server 都没有 → 必须降级为 NotAdded。
        // 否则会报 CheckBeforeUsing，用户读成「迁完了，只是语义要核对」，
        // 实际是 0 个迁成功（实测：workbuddy 的 mcp.json 9 个 server 全 disabled，
        // 迁到 dsh 全部跳过，报告却只有一行 Check before using）。
        if surface.kind == SurfaceKind::Mcp && status != ReportStatus::NotAdded {
            let converted = count_servers_in_conversion(target.mcp_format(), &conversion.content);
            if converted == 0 {
                status = ReportStatus::NotAdded;
                notes = if conversion.manual_notes.is_empty() {
                    "转换结果不含任何 MCP server，未写入目标".to_string()
                } else {
                    format!(
                        "转换结果不含任何 MCP server，未写入目标（{}）",
                        summarize_notes(&conversion.manual_notes)
                    )
                };
            }
        }

        // Added 或 CheckBeforeUsing 且 confirm_write 时写入
        // CheckBeforeUsing 也写入（转换器已生成 MANUAL 块内嵌审查提示），只有 NotAdded 不写入
        let should_write = status != ReportStatus::NotAdded && options.confirm_write;
        if should_write {
            let target_path = target.target_path(surface.kind, &surface.source_path);
            let mut final_content = conversion;
            // MCP 写入契约（v0.3）: 一律非破坏性合并，绝不整文件覆盖。
            //   - TOML 目标（Codex config.toml）→ 段级合并，保留 model/provider/API key 等其余段
            //   - JSON 目标（.mcp.json / mcp.json / mcp-settings.json）→ 合并 mcpServers，保留已有 server
            //   - cordis YAML 目标（dsh cordis.patch.yml）→ 注释托管块 upsert，
            //     块外条目/注释/`!!js` 表达式逐字保留（解析再序列化会摧毁它们）
            // 格式由布局声明（`TargetAdapter::mcp_format`），不按扩展名猜。
            if surface.kind == SurfaceKind::Mcp {
                final_content = match target.mcp_format() {
                    McpFormat::CordisYamlPluginList => {
                        merge_mcp_cordis_into_target(&target_path, final_content)?
                    }
                    McpFormat::Toml => merge_mcp_toml_into_target(&target_path, final_content)?,
                    McpFormat::Json => merge_mcp_into_target(&target_path, final_content)?,
                };
            }
            // workbuddy instructions：全文写入用户级 rules/，MEMORY.md 只保留托管块引用
            if surface.kind == SurfaceKind::Instructions && target.name() == "workbuddy" {
                final_content =
                    write_workbuddy_rules_reference(target, &surface.source_path, final_content)?;
            }
            // instructions 走托管块：保护目标文件中的手写内容
            // 手动迁移包（豆包）例外: 产物供人整段粘贴到客户端 UI，托管块标记会成为噪音
            if surface.kind == SurfaceKind::Instructions && !target.is_manual_bundle() {
                final_content = wrap_managed_block(
                    &target_path,
                    target.name(),
                    surface.kind.as_str(),
                    final_content,
                )?;
            }
            target
                .write(&target_path, &final_content)
                .context(format!("写入目标失败: {}", target_path.display()))?;

            // skills: 复制 references/scripts/assets 等支持子目录
            if surface.kind == SurfaceKind::Skills {
                copy_skill_support_dirs(source, &surface.source_path, &target_path);
            }
        }

        // 报告附带目标路径（仅对会写入的配置面；NotAdded 不显示避免误导）
        let target_hint = if status != ReportStatus::NotAdded {
            format!(
                " → {}",
                target.target_path(surface.kind, &surface.source_path).display()
            )
        } else {
            String::new()
        };
        reports.push(ReportRow::new(
            options.scope.as_str(),
            status,
            format!(
                "{} {}{}",
                surface.kind.as_str(),
                surface.source_path,
                target_hint
            ),
            notes,
        ));
    }

    // 手动迁移包（豆包）：补一份操作说明，否则用户面对一堆文件不知道往哪粘
    if target.is_manual_bundle() && options.confirm_write {
        if let Some(dir) = target.manual_bundle_dir() {
            if let Err(e) = write_manual_bundle_readme(&dir, &reports) {
                eprintln!("[警告] 写入手动迁移说明失败 {}: {e}", dir.display());
            }
        }
    }

    Ok(reports)
}

/// 生成手动迁移包说明（豆包：客户端无文件级配置，需在 UI 粘贴）
fn write_manual_bundle_readme(dir: &std::path::Path, reports: &[ReportRow]) -> anyhow::Result<()> {
    let mut md = String::new();
    md.push_str("# 豆包手动迁移包\n\n");
    md.push_str(
        "豆包桌面客户端**没有**文件级配置契约：MCP「自定义连接器」、技能与规则都存在客户端\n\
         内部存储（`%LOCALAPPDATA%\\Doubao\\User Data\\Default\\Local Storage\\leveldb`），\n\
         只能由用户在界面粘贴。因此 migrator 把转换结果整理成本目录下的「粘贴件」，\n\
         目标程序**不会**读取这些文件。\n\n",
    );
    md.push_str("## 操作步骤\n\n");
    md.push_str(
        "1. `connectors.json`：打开 豆包客户端 → 左侧「插件 · 技能 · 伙伴」→ 插件页右上角「+ 添加」\n\
         → 「新建自定义连接器」，把 JSON 里 `mcpServers` 下的每个 server 逐个填入（名称 / 命令 / 参数 / 环境变量 / URL / 请求头）。\n\
         或直接把单个 server 的对象粘给豆包，让它按 JSON 结构引导你逐项填写。\n\
         2. `instructions.md`：粘贴到豆包的自定义指示 / 人设 / 规则输入框。\n\
         3. `skills/`、`agents/`：豆包技能走技能市场，无法文件导入；按内容在「技能」里手动新建或直接作为对话附件使用。\n\n",
    );
    md.push_str("## 本次迁移清单\n\n");
    md.push_str("| 状态 | 配置面 | 说明 |\n|---|---|---|\n");
    for r in reports {
        md.push_str(&format!(
            "| {} | {} | {} |\n",
            r.status.as_str(),
            r.item.replace('|', "\\|"),
            r.notes.replace('|', "\\|")
        ));
    }
    md.push_str(
        "\n> 提示: 本目录由 migrator 生成，重复迁移会覆盖；需要保留请自行复制一份。\n",
    );
    std::fs::create_dir_all(dir)?;
    crate::safety::atomic::atomic_write(&dir.join("README.md"), &md)?;
    Ok(())
}

/// mcp merge（cordis YAML 目标，dsh）: 注释托管块 upsert
///
/// 保护用户真实 `~/.dsh/profiles/<p>/cordis.patch.yml`:
///   - 块外条目、注释、`!!js` 表达式逐字保留（不解析再序列化）
///   - 重复迁移重写同一块（幂等），不叠加第二块
///   - 块外已有同名 `id: mcp-<name>` → 跳过该 server 并记入报告（dsh 重复 id 会加载失败）
fn merge_mcp_cordis_into_target(
    target_path: &std::path::Path,
    result: ConversionResult,
) -> anyhow::Result<ConversionResult> {
    let existing = std::fs::read_to_string(target_path).unwrap_or_default();
    let upsert = mcp_yaml::upsert_cordis_mcp_block(&existing, &result.content).map_err(|e| {
        anyhow::anyhow!(
            "合并 cordis patch 失败，已拒绝写入 {}: {e}",
            target_path.display()
        )
    })?;
    let mut notes = result.manual_notes;
    let skipped = !upsert.skipped.is_empty();
    for name in &upsert.skipped {
        notes.push(format!(
            "server {name}: 目标 patch 块外已存在同名 id，已跳过（避免 dsh 重复 id 加载失败）"
        ));
    }
    Ok(ConversionResult {
        content: upsert.content,
        manual_review_required: result.manual_review_required || skipped,
        manual_notes: notes,
    })
}

/// 按配置面类型执行内容转换
///
/// - instructions: 直接复制（Exact 映射）
/// - mcp: 按 source×target 组合选格式（Codex TOML → JSON / 国产 JSON → Codex TOML / JSON→JSON）
/// - skills: 走 skills 转换器（字段降级 + MANUAL 块）
/// - agents: 走 agents 转换器；反向到 Codex 时 md→TOML
/// - 其他: 原样复制，标记需审查
fn convert_surface(
    source_name: &str,
    target_name: &str,
    surface: &ConfigSurface,
    raw: &str,
) -> ConversionResult {
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
            // 反向迁移到 Codex：md frontmatter → TOML
            let (content, needs_review, manual_notes) = if target_name == "codex" {
                crate::converter::converters::agents::convert_agent_to_toml(raw)
            } else {
                convert_agent(raw)
            };
            ConversionResult {
                content,
                manual_review_required: needs_review,
                manual_notes,
            }
        }
        SurfaceKind::Mcp => {
            // 路由顺序警告：match 命中第一条就返回，因此**必须先列最具体的组合**。
            // 曾经把 `("codex", _)` 写在 `(_, "dsh")` 之前，导致 codex → dsh 实际把
            // JSON 直接喂给 cordis YAML upsert，托管块根本没生成（静默丢配置）。
            let (content, needs_review, manual_notes) = match (source_name, target_name) {
                // Codex（TOML）→ Codex（TOML）：同格式自转，原样保留
                ("codex", "codex") => (raw.to_string(), false, Vec::new()),
                // Codex（TOML）→ dsh（cordis YAML）：TOML→JSON→cordis 两段接力
                ("codex", "dsh") => {
                    let (json, json_review, mut notes) =
                        crate::converter::converters::mcp::convert_codex_mcp(raw);
                    let (yaml, yaml_review, yaml_notes) =
                        mcp_yaml::convert_json_mcp_to_cordis(&json);
                    notes.extend(yaml_notes);
                    (yaml, json_review || yaml_review, notes)
                }
                // Codex 源（TOML）→ 其余目标（JSON / TOML 由 merge 层落盘）
                ("codex", _) => crate::converter::converters::mcp::convert_codex_mcp(raw),
                // 国产/Claude JSON 源 → Codex TOML 目标（反向）
                (_, "codex") => {
                    crate::converter::converters::mcp::convert_mcp_json_to_toml(raw)
                }
                // JSON 源 → dsh：cordis patch YAML 插件条目
                (_, "dsh") => mcp_yaml::convert_json_mcp_to_cordis(raw),
                // JSON → JSON（claude/qoder/trae/lingma/workbuddy/doubao 之间）：直接复制，merge 由管线处理
                _ => {
                    let needs_review = false;
                    let manual_notes = Vec::new();
                    (raw.to_string(), needs_review, manual_notes)
                }
            };
            ConversionResult {
                content,
                manual_review_required: needs_review,
                manual_notes,
            }
        }
        SurfaceKind::Commands => {
            // Claude slash 命令 → 单文件 skill（name 取文件名，如 review.md → review）
            let name = std::path::Path::new(&surface.source_path)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "command".to_string());
            let (content, needs_review, manual_notes) =
                crate::converter::converters::commands::convert_command(raw, &name);
            ConversionResult {
                content,
                manual_review_required: needs_review,
                manual_notes,
            }
        }
        // instructions / 其他: 直接复制
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

/// 判断目标是否为 TOML 配置（Codex 的 `~/.codex/config.toml`）
///
/// 契约: 只看扩展名，不看文件名关键字 —— 避免再次落入"字符串猜类型"的坑
fn is_toml_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("toml"))
        .unwrap_or(false)
}

/// mcp merge（TOML 目标）: 段级合并 `[mcp_servers.*]`，逐字保留其余段
///
/// 这是保护用户真实 `~/.codex/config.toml` 的关键路径：
///   - 目标不存在 / 为空 → 直接写入迁移产物
///   - 目标存在 → 只增改 `[mcp_servers.*]`，model/provider/API key 等其余段原样保留
///   - 目标 TOML 损坏或 mcp_servers 为 inline table → 返回错误，**拒绝写入**（fail-closed）
fn merge_mcp_toml_into_target(
    target_path: &std::path::Path,
    result: ConversionResult,
) -> anyhow::Result<ConversionResult> {
    let existing = std::fs::read_to_string(target_path).unwrap_or_default();
    if existing.trim().is_empty() {
        return Ok(result);
    }
    let merged = crate::converter::converters::mcp::merge_mcp_servers_toml(&existing, &result.content)
        .map_err(|e| {
            anyhow::anyhow!(
                "合并 MCP 配置失败，已拒绝写入 {}: {e}",
                target_path.display()
            )
        })?;
    Ok(ConversionResult {
        content: merged,
        manual_review_required: result.manual_review_required,
        manual_notes: result.manual_notes,
    })
}

/// mcp merge: 目标文件已存在 JSON 时合并 mcpServers（保留已有 server，同名以迁移内容为准）
fn merge_mcp_into_target(target_path: &std::path::Path, result: ConversionResult) -> anyhow::Result<ConversionResult> {
    let existing = std::fs::read_to_string(target_path).unwrap_or_default();
    if existing.trim().is_empty() {
        return Ok(result);
    }
    let merged = merge_mcp_servers_json(&existing, &result.content)?;
    Ok(ConversionResult {
        content: merged,
        manual_review_required: result.manual_review_required,
        manual_notes: result.manual_notes,
    })
}

/// 合并两份 mcpServers JSON（保留 base 已有 server，同名 key 以 incoming 为准）
fn merge_mcp_servers_json(base: &str, incoming: &str) -> anyhow::Result<String> {
    let mut out: serde_json::Value =
        serde_json::from_str(base).context("解析已有 MCP 配置失败")?;
    if !out.is_object() {
        out = serde_json::json!({});
    }
    let inc: serde_json::Value =
        serde_json::from_str(incoming).context("解析迁移 MCP 配置失败")?;
    let inc_servers = inc
        .get("mcpServers")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));

    let out_servers = out
        .as_object_mut()
        .expect("forced object above")
        .entry("mcpServers".to_string())
        .or_insert_with(|| serde_json::json!({}));
    if !out_servers.is_object() {
        *out_servers = serde_json::json!({});
    }
    let out_map = out_servers.as_object_mut().expect("forced object");
    if let Some(inc_map) = inc_servers.as_object() {
        for (k, v) in inc_map {
            out_map.insert(k.clone(), v.clone());
        }
    }
    Ok(serde_json::to_string_pretty(&out)?)
}

/// workbuddy instructions 额外落地的规则全文路径: `~/.workbuddy/rules/<source>-rules.md`
///
/// 集中定义，供写入（`write_workbuddy_rules_reference`）与备份（main.rs apply）共用，
/// 避免两处逻辑漂移导致"写入了却没备份"或反之。
pub fn workbuddy_rules_path<T: TargetAdapter>(
    target: &T,
    source_path: &str,
) -> PathBuf {
    let source_name = if source_path.contains(".codex") {
        "codex"
    } else {
        "claude"
    };
    target
        .user_dir()
        .join("rules")
        .join(format!("{source_name}-rules.md"))
}

/// workbuddy instructions: 全文写入 ~/.workbuddy/rules/<source>-rules.md，
/// 返回的 content 仅保留简短引用（避免数百行规则塞爆记忆文件容量）。
fn write_workbuddy_rules_reference<T: TargetAdapter>(
    target: &T,
    source_path: &str,
    result: ConversionResult,
) -> anyhow::Result<ConversionResult> {
    let rules_file = workbuddy_rules_path(target, source_path);
    if let Some(parent) = rules_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::safety::atomic::atomic_write(&rules_file, &result.content)
        .context(format!("写入规则文件失败: {}", rules_file.display()))?;
    let line_count = result.content.lines().count();
    let reference = format!(
        "源平台全局规则已迁移，全文位于 `{path}`（{lines} 行）。\n处理 Java/SpringBoot/PostgreSQL/中文编码等任务时，请先读取该规则文件。\n源: `{src}`",
        path = rules_file.display(),
        lines = line_count,
        src = source_path,
    );
    Ok(ConversionResult {
        content: reference,
        manual_review_required: result.manual_review_required,
        manual_notes: result.manual_notes,
    })
}

/// 托管块 scope — 按「目标平台 + 配置面类型」稳定标识
///
/// 契约（P1 #6 修复）: scope 必须跨迁移方向稳定。旧实现用 `migrator:<source_path>`，
/// 同一目标文件先后从两个源迁移会产生两个不同 scope → 叠加多块、重复迁移不更新。
/// 改为 `migrator:<target_name>:<kind>` 后：无论源是哪个，落到同一目标文件的同一配置面
/// 永远共享同一个托管块，重复迁移只重写该块而非新增。
fn managed_block_scope(target_name: &str, kind: &str) -> String {
    format!("migrator:{target_name}:{kind}")
}

/// 用托管块包裹 instructions 内容：保护目标文件已有手写内容
///
/// scope 按目标平台 + 配置面类型稳定标识（见 `managed_block_scope`）；若目标文件已有托管块则重写，否则追加
/// 契约: 标记异常（重复/缺半/顺序反）→ 返回错误，拒绝写入（不尽力合并）
fn wrap_managed_block(
    target_path: &std::path::Path,
    target_name: &str,
    kind: &str,
    conversion: ConversionResult,
) -> anyhow::Result<ConversionResult> {
    let scope = managed_block_scope(target_name, kind);
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

/// 统计目标 MCP 配置中的 server 数量（TOML 看 `mcp_servers`，JSON 看 `mcpServers`）
///
/// 解析失败（损坏/非预期格式）返回 0 —— 由调用方标记为需人工审查。
/// 把转换器报的多条提示压成一行（报告列宽有限，但不能只显示第一条）
///
/// ≤3 条全部列出；更多则列前 2 条 + 总数，完整清单看 `plan` / `dry-run`。
fn summarize_notes(notes: &[String]) -> String {
    const MAX_INLINE: usize = 3;
    if notes.len() <= MAX_INLINE {
        notes.join("；")
    } else {
        format!(
            "{}；……（共 {} 条，完整清单见 plan / dry-run 输出）",
            notes[..2].join("；"),
            notes.len()
        )
    }
}

/// 数「转换产物」里的 MCP server 数（区别于 `count_mcp_servers`：那个数目标文件）
///
/// 用于写入前的守卫：产物 0 个 server 说明全被跳过，必须报 NotAdded。
fn count_servers_in_conversion(format: McpFormat, content: &str) -> usize {
    match format {
        McpFormat::CordisYamlPluginList => content
            .lines()
            .filter(|l| l.trim_start().starts_with("- id:"))
            .count(),
        McpFormat::Toml => toml::from_str::<toml::Value>(content)
            .ok()
            .and_then(|v| {
                v.get("mcp_servers")
                    .and_then(|t| t.as_table())
                    .map(|t| t.len())
            })
            .unwrap_or(0),
        McpFormat::Json => serde_json::from_str::<serde_json::Value>(content)
            .ok()
            .and_then(|v| {
                v.get("mcpServers")
                    .and_then(|t| t.as_object())
                    .map(|t| t.len())
            })
            .unwrap_or(0),
    }
}

fn count_mcp_servers(content: &str, path: &std::path::Path) -> usize {
    if is_toml_path(path) {
        if let Ok(v) = toml::from_str::<toml::Value>(content) {
            v.get("mcp_servers")
                .and_then(|t| t.as_table())
                .map(|t| t.len())
                .unwrap_or(0)
        } else {
            0
        }
    } else if let Ok(v) = serde_json::from_str::<serde_json::Value>(content) {
        v.get("mcpServers")
            .and_then(|t| t.as_object())
            .map(|t| t.len())
            .unwrap_or(0)
    } else {
        0
    }
}

/// 阶段 5: 校验目标（内容级，非仅存在性）
///
/// - Instructions: 校验托管块存在且非空；若目标为 workbuddy 额外校验 `rules/` 全文落地
/// - Mcp: 校验目标含至少 1 个 server（数量来自真正的 TOML/JSON 解析）
/// - 其余面: 仅校验存在性（向后兼容）
///
/// 校验失败时降级为 `CheckBeforeUsing` 而非 `NotAdded`，便于用户区分"文件缺失"与"内容异常"。
pub fn validate_target<T: TargetAdapter>(target: &T, surfaces: &[ConfigSurface]) -> Vec<ReportRow> {
    let mut reports = Vec::new();
    for surface in surfaces {
        if !surface.present {
            continue;
        }
        let path = target.target_path(surface.kind, &surface.source_path);
        // 目标文件缺失：直接报 NotAdded，跳过内容级检查
        if !path.exists() {
            reports.push(ReportRow::new(
                "validate",
                ReportStatus::NotAdded,
                format!("{} target {}", surface.kind.as_str(), path.display()),
                "missing",
            ));
            continue;
        }
        let mut notes: Vec<String> = Vec::new();
        let mut status = ReportStatus::Added;

        match surface.kind {
            SurfaceKind::Instructions => {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    // 手动迁移包（豆包）**按设计不加托管块**：产物是给人整段粘进客户端 UI 的，
                    // 标记只会变成噪音。写入端用 is_manual_bundle() 跳过了包裹，校验端必须
                    // 同步跳过，否则每次 apply 成功后 validate 都假报「托管块缺失」。
                    if target.is_manual_bundle() {
                        if content.trim().is_empty() {
                            notes.push("粘贴件为空".into());
                            status = ReportStatus::CheckBeforeUsing;
                        } else {
                            notes.push("手动粘贴件非空(该目标不使用托管块)".into());
                        }
                    } else {
                        let scope = managed_block_scope(target.name(), surface.kind.as_str());
                        match crate::safety::managed_block::extract_managed_block(&content, &scope) {
                            Some(body) if !body.trim().is_empty() => {
                                notes.push("托管块存在且非空".into());
                            }
                            Some(_) => {
                                notes.push("托管块存在但为空".into());
                                status = ReportStatus::CheckBeforeUsing;
                            }
                            None => {
                                // 兼容旧契约 scope：早期版本用 `migrator:<source_path>`，
                                // 用户真实文件里普遍残留。直接报「缺失」会让用户以为要重迁，
                                // 实际下次 apply 会自动就地升级为当前 scope。
                                match crate::safety::managed_block::find_any_migrator_block(
                                    &content,
                                ) {
                                    Some(legacy) => {
                                        notes.push(format!(
                                            "检测到旧版 scope 托管块({legacy})，下次 apply 会自动升级为 {scope}"
                                        ));
                                    }
                                    None => {
                                        notes.push("托管块缺失(目标未含 migrator 同步块)".into());
                                        status = ReportStatus::CheckBeforeUsing;
                                    }
                                }
                            }
                        }
                    }
                }
                // workbuddy 额外检查 rules 全文
                if target.name() == "workbuddy" {
                    let rules = workbuddy_rules_path(target, &surface.source_path);
                    if rules.exists() {
                        notes.push(format!("rules 全文存在: {}", rules.display()));
                    } else {
                        notes.push("rules 全文缺失(~/.workbuddy/rules/ 未写入)".into());
                        status = ReportStatus::CheckBeforeUsing;
                    }
                }
            }
            SurfaceKind::Mcp => {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    // 计数方式随目标格式: cordis patch 数托管块内的插件条目，其余数 mcpServers/mcp_servers
                    let count = match target.mcp_format() {
                        McpFormat::CordisYamlPluginList => mcp_yaml::extract_cordis_mcp_block(&content)
                            .map(|b| {
                                b.lines()
                                    .filter(|l| l.trim_start().starts_with("- id:"))
                                    .count()
                            })
                            .unwrap_or(0),
                        _ => count_mcp_servers(&content, &path),
                    };
                    if count == 0 {
                        notes.push("目标 MCP 配置未含任何 server".into());
                        status = ReportStatus::CheckBeforeUsing;
                    } else {
                        notes.push(format!("含 {count} 个 MCP server"));
                    }
                }
            }
            _ => {}
        }

        reports.push(ReportRow::new(
            "validate",
            status,
            format!("{} target {}", surface.kind.as_str(), path.display()),
            if notes.is_empty() {
                "exists".into()
            } else {
                notes.join("; ")
            },
        ));
    }
    reports
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety::managed_block::{begin_marker, extract_managed_block};
    use crate::target::TargetWriteResult;

    #[test]
    fn merge_mcp_servers_preserves_existing_and_overrides_same_name() {
        let base = r#"{"mcpServers":{"keep-me":{"command":"x"},"same":{"command":"old"}}}"#;
        let incoming = r#"{"mcpServers":{"same":{"command":"new"},"fresh":{"command":"y"}}}"#;
        let merged = merge_mcp_servers_json(base, incoming).unwrap();
        let v: serde_json::Value = serde_json::from_str(&merged).unwrap();
        let servers = v["mcpServers"].as_object().unwrap();
        assert_eq!(servers.len(), 3);
        assert!(servers.contains_key("keep-me"), "已有 server 必须保留");
        assert_eq!(servers["same"]["command"], "new", "同名以迁移内容为准");
        assert!(servers.contains_key("fresh"), "新 server 加入");
    }

    #[test]
    fn merge_mcp_servers_tolerates_missing_mcp_servers_key() {
        // 目标已有 JSON 但不含 mcpServers → 正常合并，不 panic
        let merged = merge_mcp_servers_json(
            r#"{"other":"value"}"#,
            r#"{"mcpServers":{"a":{"command":"x"}}}"#,
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(v["mcpServers"]["a"]["command"], "x");
        assert_eq!(v["other"], "value");
    }

    #[test]
    fn merge_mcp_into_target_passthrough_when_no_existing_file() {
        let path = std::env::temp_dir().join(format!("migrator-merge-none-{}", std::process::id()));
        let result = ConversionResult {
            content: r#"{"mcpServers":{"a":{"command":"x"}}}"#.into(),
            manual_review_required: false,
            manual_notes: vec![],
        };
        // 目标不存在 → 直接返回原内容（不报错）
        let out = merge_mcp_into_target(&path, result.clone()).unwrap();
        assert_eq!(out.content, result.content);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn is_toml_path_only_matches_extension_not_filename_keyword() {
        assert!(is_toml_path(std::path::Path::new("/home/u/.codex/config.toml")));
        assert!(is_toml_path(std::path::Path::new("/x/CONFIG.TOML")));
        // 关键：名字里带 toml 但扩展名是 json 的，绝不能当 TOML 处理
        assert!(!is_toml_path(std::path::Path::new("/x/toml-notes.json")));
        assert!(!is_toml_path(std::path::Path::new("/x/.mcp.json")));
        assert!(!is_toml_path(std::path::Path::new("/x/mcp")));
    }

    /// 回归（P0-2 核心）: 迁移 MCP 到 Codex 时，config.toml 的其余段必须逐字保留
    ///
    /// 这条测试守的就是"不要覆盖我 Codex 的真实配置"——
    /// model / model_provider / API key 等段一旦丢失即为数据损毁。
    #[test]
    fn merge_mcp_toml_preserves_unrelated_codex_sections() {
        let dir = std::env::temp_dir().join(format!(
            "migrator-toml-merge-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let existing = "\
model = \"o3\"
model_provider = \"openai\"

# 我手写的注释，必须保留
[model_providers.openai]
name = \"OpenAI\"
env_key = \"OPENAI_API_KEY\"

[mcp_servers.keep-me]
command = \"keep\"
";
        std::fs::write(&path, existing).unwrap();

        let incoming = ConversionResult {
            content: "[mcp_servers.fresh]\ncommand = \"npx\"\n".into(),
            manual_review_required: false,
            manual_notes: vec![],
        };
        let out = merge_mcp_toml_into_target(&path, incoming).unwrap();

        assert!(out.content.contains("model = \"o3\""), "model 段必须保留");
        assert!(
            out.content.contains("env_key = \"OPENAI_API_KEY\""),
            "provider 段必须保留"
        );
        assert!(
            out.content.contains("# 我手写的注释，必须保留"),
            "注释必须保留"
        );
        assert!(
            out.content.contains("[mcp_servers.keep-me]"),
            "已有 server 必须保留"
        );
        assert!(
            out.content.contains("[mcp_servers.fresh]"),
            "新 server 必须写入"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 回归（fail-closed）: 目标 config.toml 损坏时必须报错拒绝写入，而不是覆盖
    #[test]
    fn merge_mcp_toml_refuses_to_write_when_target_is_corrupt() {
        let dir = std::env::temp_dir().join(format!(
            "migrator-toml-corrupt-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "this is [not valid toml ===\n").unwrap();

        let incoming = ConversionResult {
            content: "[mcp_servers.fresh]\ncommand = \"npx\"\n".into(),
            manual_review_required: false,
            manual_notes: vec![],
        };
        let err = merge_mcp_toml_into_target(&path, incoming).unwrap_err();
        assert!(
            err.to_string().contains("拒绝写入"),
            "损坏目标必须 fail-closed，实际: {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_mcp_toml_passthrough_when_target_missing() {
        let path = std::env::temp_dir().join(format!(
            "migrator-toml-none-{}-{}.toml",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_file(&path);
        let incoming = ConversionResult {
            content: "[mcp_servers.fresh]\ncommand = \"npx\"\n".into(),
            manual_review_required: false,
            manual_notes: vec![],
        };
        let out = merge_mcp_toml_into_target(&path, incoming.clone()).unwrap();
        assert_eq!(out.content, incoming.content);
    }

    /// 测试用桩目标：按配置面类型映射到指定文件，user_dir 可配置
    struct PathStub {
        name: &'static str,
        files: std::collections::HashMap<&'static str, std::path::PathBuf>,
        user_dir: std::path::PathBuf,
    }
    impl TargetAdapter for PathStub {
        fn name(&self) -> &'static str {
            self.name
        }
        fn target_path(&self, k: SurfaceKind, _p: &str) -> std::path::PathBuf {
            self.files
                .get(k.as_str())
                .cloned()
                .unwrap_or_else(std::path::PathBuf::new)
        }
        fn user_dir(&self) -> std::path::PathBuf {
            self.user_dir.clone()
        }
        fn write(
            &self,
            path: &std::path::Path,
            result: &ConversionResult,
        ) -> std::io::Result<TargetWriteResult> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, &result.content)?;
            Ok(TargetWriteResult {
                path: path.to_path_buf(),
                created: true,
            })
        }
    }

    /// 回归（P1 #5）: rules 路径计算正确且按源标识分文件
    #[test]
    fn workbuddy_rules_path_uses_user_dir_and_source_name() {
        let t = PathStub {
            name: "workbuddy",
            files: std::collections::HashMap::new(),
            user_dir: std::path::PathBuf::from("/home/u/.workbuddy"),
        };
        let codex = workbuddy_rules_path(&t, "/home/u/.codex/AGENTS.md");
        assert_eq!(
            codex,
            std::path::PathBuf::from("/home/u/.workbuddy/rules/codex-rules.md")
        );
        let claude = workbuddy_rules_path(&t, "/home/u/.claude/CLAUDE.md");
        assert_eq!(
            claude,
            std::path::PathBuf::from("/home/u/.workbuddy/rules/claude-rules.md")
        );
    }

    /// 回归（P1 #6 核心）: 同一目标 + 同一面 → 唯一稳定 scope，与源路径无关
    #[test]
    fn managed_block_scope_is_stable_across_sources() {
        assert_eq!(
            managed_block_scope("qoder", "instructions"),
            "migrator:qoder:instructions"
        );
        // 两个不同源路径必须落到同一个 scope（旧实现会因 source_path 不同而分叉）
        let from_a = managed_block_scope("qoder", "instructions");
        let from_b = managed_block_scope("qoder", "instructions");
        assert_eq!(from_a, from_b);
    }

    /// 回归（P1 #6）: 重复迁移更新同一托管块，不可叠加第二个块
    #[test]
    fn wrap_managed_block_reuses_same_scope_on_repeat_migration() {
        let dir = std::env::temp_dir().join(format!(
            "migrator-scope-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("AGENTS.md");
        let v1 = wrap_managed_block(
            &path,
            "qoder",
            "instructions",
            ConversionResult {
                content: "v1".into(),
                manual_review_required: false,
                manual_notes: vec![],
            },
        )
        .unwrap();
        assert!(v1
            .content
            .contains(&begin_marker("migrator:qoder:instructions")));
        std::fs::write(&path, &v1.content).unwrap();
        // 第二次迁移（即便是不同源）应更新同一块，不得叠加第二个块
        let v2 = wrap_managed_block(
            &path,
            "qoder",
            "instructions",
            ConversionResult {
                content: "v2".into(),
                manual_review_required: false,
                manual_notes: vec![],
            },
        )
        .unwrap();
        let marker = begin_marker("migrator:qoder:instructions");
        assert_eq!(
            v2.content.matches(&marker).count(),
            1,
            "重复迁移必须更新同一托管块，不可叠加"
        );
        let body = extract_managed_block(&v2.content, "migrator:qoder:instructions").unwrap();
        assert_eq!(body, "v2");
        assert!(!v2.content.contains("v1"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 回归（P1 #7）: validate 检出托管块存在 / 缺失
    #[test]
    fn validate_reports_managed_block_present_and_missing() {
        let dir = std::env::temp_dir().join(format!(
            "migrator-val-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let inst = dir.join("AGENTS.md");
        let with_block = wrap_managed_block(
            &inst,
            "qoder",
            "instructions",
            ConversionResult {
                content: "synced".into(),
                manual_review_required: false,
                manual_notes: vec![],
            },
        )
        .unwrap();
        std::fs::write(&inst, &with_block.content).unwrap();
        let bare = dir.join("AGENTS2.md");
        std::fs::write(&bare, "# 手写\n").unwrap();

        let mut files = std::collections::HashMap::new();
        files.insert("instructions", inst.clone());
        let stub = PathStub {
            name: "qoder",
            files,
            user_dir: dir.clone(),
        };
        let surfaces = vec![ConfigSurface::new(SurfaceKind::Instructions, "x/AGENTS.md", true)];
        let reports = validate_target(&stub, &surfaces);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].status, ReportStatus::Added);
        assert!(reports[0].notes.contains("托管块存在且非空"));

        let mut files2 = std::collections::HashMap::new();
        files2.insert("instructions", bare.clone());
        let stub2 = PathStub {
            name: "qoder",
            files: files2,
            user_dir: dir.clone(),
        };
        let reports2 = validate_target(&stub2, &surfaces);
        assert_eq!(reports2[0].status, ReportStatus::CheckBeforeUsing);
        assert!(reports2[0].notes.contains("托管块缺失"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 回归（P1 #7）: validate 校验 MCP server 数量，空配置降级
    #[test]
    fn validate_reports_mcp_server_count() {
        let dir = std::env::temp_dir().join(format!(
            "migrator-valmcp-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mcp = dir.join("mcp.json");
        std::fs::write(
            &mcp,
            r#"{"mcpServers":{"a":{"command":"x"},"b":{"command":"y"}}}"#,
        )
        .unwrap();
        let mut files = std::collections::HashMap::new();
        files.insert("mcp", mcp.clone());
        let stub = PathStub {
            name: "qoder",
            files,
            user_dir: dir.clone(),
        };
        let surfaces = vec![ConfigSurface::new(SurfaceKind::Mcp, "mcp.json", true)];
        let reports = validate_target(&stub, &surfaces);
        assert_eq!(reports[0].status, ReportStatus::Added);
        assert!(reports[0].notes.contains("含 2 个 MCP server"));

        let mcp_empty = dir.join("empty.json");
        std::fs::write(&mcp_empty, r#"{"mcpServers":{}}"#).unwrap();
        let mut files2 = std::collections::HashMap::new();
        files2.insert("mcp", mcp_empty.clone());
        let stub2 = PathStub {
            name: "qoder",
            files: files2,
            user_dir: dir.clone(),
        };
        let reports2 = validate_target(&stub2, &surfaces);
        assert_eq!(reports2[0].status, ReportStatus::CheckBeforeUsing);
        assert!(reports2[0].notes.contains("未含任何 server"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 回归（真机实测发现）: codex → dsh 的 MCP 必须落到 cordis YAML 条目。
    ///
    /// 曾经的缺陷：`match (source, target)` 把 `("codex", _)` 写在 `(_, "dsh")` 之前，
    /// 命中即返回，于是 codex → dsh 把 TOML 转出的 **JSON** 直接交给 cordis upsert，
    /// 托管块一条都没生成 —— 真机 `apply` 退出码 0、文件 mtime 变了，但 MCP 配置静默
    /// 丢失。这条测试锁死路由顺序。
    #[test]
    fn codex_mcp_to_dsh_routes_through_cordis_converter() {
        let toml = "\
[mcp_servers.ctx7]
command = \"npx\"
args = [\"-y\", \"@upstash/context7-mcp\"]

[mcp_servers.gh]
url = \"https://api.githubcopilot.com/mcp/\"
bearer_token_env_var = \"GITHUB_TOKEN\"
";
        let surface = ConfigSurface::new(SurfaceKind::Mcp, "~/.codex/config.toml", true);
        let out = convert_surface("codex", "dsh", &surface, toml);

        assert!(
            out.content.trim_start().starts_with("- id:"),
            "dsh 目标必须是 cordis YAML 条目列表，实际开头: {:?}",
            out.content.lines().next()
        );
        assert!(
            !out.content.trim_start().starts_with('{'),
            "绝不能把 JSON 直接喂给 cordis upsert"
        );
        assert_eq!(
            out.content.matches("- id: mcp-").count(),
            2,
            "两个 server 都应转换，实际内容:\n{}",
            out.content
        );
        assert!(
            out.content.contains("'@deepseek-ai/dsh-mcp-client'"),
            "每条必须是 dsh-mcp-client 插件条目"
        );
        assert!(
            out.content.contains("transport: streamable-http"),
            "url 型 server 必须走 streamable-http"
        );
    }

    /// codex → codex 是 TOML 自转，必须原样保留，不能被转成 JSON
    #[test]
    fn codex_mcp_to_codex_is_passthrough() {
        let toml = "[mcp_servers.a]\ncommand = \"npx\"\n";
        let surface = ConfigSurface::new(SurfaceKind::Mcp, "~/.codex/config.toml", true);
        let out = convert_surface("codex", "codex", &surface, toml);
        assert_eq!(out.content, toml);
    }

    /// 回归（真机发现）: 豆包手动迁移包按设计不加托管块，
    /// validate 必须同步跳过该项检查，否则刚 apply 成功就假报「托管块缺失」。
    #[test]
    fn validate_skips_managed_block_check_for_manual_bundle() {
        let dir = std::env::temp_dir().join(format!(
            "migrator-valbundle-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let inst = dir.join("instructions.md");
        std::fs::write(&inst, "粘进豆包的规则正文\n").unwrap();

        struct BundleStub {
            path: std::path::PathBuf,
        }
        impl TargetAdapter for BundleStub {
            fn name(&self) -> &'static str {
                "doubao"
            }
            fn target_path(&self, _k: SurfaceKind, _p: &str) -> std::path::PathBuf {
                self.path.clone()
            }
            fn user_dir(&self) -> std::path::PathBuf {
                std::path::PathBuf::new()
            }
            fn is_manual_bundle(&self) -> bool {
                true
            }
            fn write(
                &self,
                _path: &std::path::Path,
                _result: &ConversionResult,
            ) -> std::io::Result<TargetWriteResult> {
                unimplemented!()
            }
        }

        let stub = BundleStub { path: inst };
        let surfaces = vec![ConfigSurface::new(
            SurfaceKind::Instructions,
            "instructions.md",
            true,
        )];
        let reports = validate_target(&stub, &surfaces);
        assert_eq!(
            reports[0].status,
            ReportStatus::Added,
            "手动包不应因缺托管块降级，实际 notes: {}",
            reports[0].notes
        );
        assert!(
            reports[0].notes.contains("不使用托管块"),
            "实际: {}",
            reports[0].notes
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 回归: 空的手动粘贴件仍要报警
    #[test]
    fn validate_flags_empty_manual_bundle() {
        let dir = std::env::temp_dir().join(format!(
            "migrator-valbundle-empty-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let inst = dir.join("instructions.md");
        std::fs::write(&inst, "   \n").unwrap();

        struct EmptyBundleStub {
            path: std::path::PathBuf,
        }
        impl TargetAdapter for EmptyBundleStub {
            fn name(&self) -> &'static str {
                "doubao"
            }
            fn target_path(&self, _k: SurfaceKind, _p: &str) -> std::path::PathBuf {
                self.path.clone()
            }
            fn user_dir(&self) -> std::path::PathBuf {
                std::path::PathBuf::new()
            }
            fn is_manual_bundle(&self) -> bool {
                true
            }
            fn write(
                &self,
                _path: &std::path::Path,
                _result: &ConversionResult,
            ) -> std::io::Result<TargetWriteResult> {
                unimplemented!()
            }
        }

        let stub = EmptyBundleStub { path: inst };
        let surfaces = vec![ConfigSurface::new(
            SurfaceKind::Instructions,
            "instructions.md",
            true,
        )];
        let reports = validate_target(&stub, &surfaces);
        assert_eq!(reports[0].status, ReportStatus::CheckBeforeUsing);
        assert!(reports[0].notes.contains("粘贴件为空"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 回归: 转换器的多条提示不能被 first() 截断
    #[test]
    fn summarize_notes_lists_every_note_when_few() {
        let notes = vec!["a 跳过".to_string(), "b 跳过".to_string()];
        let s = summarize_notes(&notes);
        assert!(s.contains("a 跳过") && s.contains("b 跳过"), "实际: {s}");
        assert!(!s.contains("共"), "少量时不该压缩");
    }

    /// 回归（真机发现）: 9 个 server 全被跳过时，旧实现只显示第 1 条，
    /// 用户误以为只跳了 1 个。现在必须给出总数。
    #[test]
    fn summarize_notes_reports_total_when_many() {
        let notes: Vec<String> = (1..=9).map(|i| format!("server s{i}: 已跳过")).collect();
        let s = summarize_notes(&notes);
        assert!(s.contains("server s1"), "应保留前几条: {s}");
        assert!(s.contains("共 9 条"), "必须报出总数: {s}");
    }

    /// 守卫依赖的计数必须对三种目标格式都准
    #[test]
    fn count_servers_in_conversion_handles_all_formats() {
        let cordis = "- id: mcp-a\n  name: x\n- id: mcp-b\n  name: y\n";
        assert_eq!(count_servers_in_conversion(McpFormat::CordisYamlPluginList, cordis), 2);
        assert_eq!(count_servers_in_conversion(McpFormat::CordisYamlPluginList, ""), 0);

        let json = r#"{"mcpServers":{"a":{"command":"x"},"b":{"command":"y"}}}"#;
        assert_eq!(count_servers_in_conversion(McpFormat::Json, json), 2);
        assert_eq!(count_servers_in_conversion(McpFormat::Json, r#"{"mcpServers":{}}"#), 0);

        let toml = "[mcp_servers.a]\ncommand = \"x\"\n[mcp_servers.b]\ncommand = \"y\"\n";
        assert_eq!(count_servers_in_conversion(McpFormat::Toml, toml), 2);
        assert_eq!(count_servers_in_conversion(McpFormat::Toml, ""), 0);
    }
}
