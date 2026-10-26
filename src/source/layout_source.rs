//! LayoutSource — 布局驱动源适配器（反向迁移：国产平台 → Claude/Codex）
//!
//! 与 LayoutTarget 镜像：用 TargetLayout 描述"从哪读"，即可把目标平台当源。
//! 支持读取 qoder/trae/lingma/workbuddy 的配置（指令/MCP/skills/agents）。
//!
//! 只读契约：本模块绝不修改源文件。

use super::SourceAdapter;
use crate::converter::converters::mcp_yaml;
use crate::model::{ConfigSurface, SurfaceKind};
use crate::safety::managed_block::strip_markers;
use crate::target::layout::{McpFormat, TargetLayout};
use std::path::{Path, PathBuf};

/// 路径转正斜杠（source_path 统一用 `/`，Windows 反斜杠会破坏前缀匹配）
fn slashed(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// 布局驱动源适配器
pub struct LayoutSource {
    layout: TargetLayout,
    user_dir: PathBuf,
    project_dir: PathBuf,
    project_root_dir: PathBuf,
}

impl LayoutSource {
    pub fn new(
        layout: TargetLayout,
        user_dir: impl Into<PathBuf>,
        project_dir: impl Into<PathBuf>,
        project_root_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            layout,
            user_dir: user_dir.into(),
            project_dir: project_dir.into(),
            project_root_dir: project_root_dir.into(),
        }
    }

    pub fn detect(layout: TargetLayout, current_dir: &Path) -> Self {
        Self::new(
            layout.clone(),
            layout.user_root(),
            layout.project_root(current_dir),
            current_dir.to_path_buf(),
        )
    }

    /// 布局目录前缀（供 source_path 使用，target 侧 strip 兼容）
    fn dir_prefix(&self) -> &'static str {
        match self.layout.name {
            "qoder" => ".qoder/",
            "trae" => ".trae/",
            "lingma" => ".lingma/",
            // dsh/doubao 的配置在用户级 / 手动包目录下，由 user_prefix / manual_bundle_dir 处理
            _ => "",
        }
    }

    /// 用户级路径前缀（如 `~/.workbuddy/` / `~/.dsh/`）
    fn user_prefix(&self) -> String {
        format!("~/{}/", self.layout.user_dir_name.trim_start_matches("~/"))
    }

    /// MCP 配置实际路径（dsh 需解析 profile 层）
    fn mcp_config_path(&self) -> PathBuf {
        self.layout
            .mcp_path(&self.user_dir, &self.project_dir, &self.project_root_dir)
    }

    /// 由 source_path 反推磁盘路径
    fn path_from_source(&self, source_path: &str) -> PathBuf {
        // 用户级前缀（~/.workbuddy/ / ~/.dsh/ ...）
        let up = self.user_prefix();
        if let Some(rest) = source_path.strip_prefix(up.as_str()) {
            return self.user_dir.join(rest);
        }
        // 项目配置根前缀（.qoder/ / .trae/ / .lingma/）
        let prefix = self.dir_prefix();
        if !prefix.is_empty()
            && let Some(rest) = source_path.strip_prefix(prefix)
        {
            return self.project_dir.join(rest);
        }
        // 手动迁移包（豆包）: 项目根下 .doubao/manual/
        if let Some(dir) = self.layout.manual_bundle_dir
            && source_path.starts_with(dir)
        {
            return self.project_root_dir.join(source_path);
        }
        if source_path.ends_with("MEMORY.md") || source_path.contains("rules/") {
            // workbuddy 用户级指令/规则
            return self
                .user_dir
                .join(source_path.trim_start_matches(up.as_str()));
        }
        // 无前缀的相对路径（如项目根 AGENTS.md）→ 项目根
        self.project_root_dir.join(source_path)
    }

    fn surface_instructions(&self) -> ConfigSurface {
        // 用户级 AGENTS.md / MEMORY.md（workbuddy / codex / dsh）
        if self.layout.instructions_at_user_root {
            let p = self.user_dir.join(self.layout.instructions_file);
            ConfigSurface::new(
                SurfaceKind::Instructions,
                format!("{}{}", self.user_prefix(), self.layout.instructions_file),
                p.exists(),
            )
        } else if let Some(dir) = self.layout.manual_bundle_dir {
            // 手动迁移包（豆包）: .doubao/manual/instructions.md
            let p = self
                .project_root_dir
                .join(dir)
                .join(self.layout.instructions_file);
            ConfigSurface::new(
                SurfaceKind::Instructions,
                format!("{dir}/{}", self.layout.instructions_file),
                p.exists(),
            )
        } else {
            let p = self.project_root_dir.join(self.layout.instructions_file);
            ConfigSurface::new(
                SurfaceKind::Instructions,
                self.layout.instructions_file.to_string(),
                p.exists(),
            )
        }
    }

    fn surface_mcp(&self) -> ConfigSurface {
        let p = self.mcp_config_path();
        // source_path 归一: 用户级 → ~/.<user>/rel；项目根下 → 相对项目根
        let sp = match p.strip_prefix(&self.user_dir) {
            Ok(rel) => format!("{}{}", self.user_prefix(), slashed(rel)),
            Err(_) => match p.strip_prefix(&self.project_root_dir) {
                Ok(rel) => slashed(rel),
                Err(_) => format!("{}{}", self.dir_prefix(), self.layout.mcp_file),
            },
        };
        ConfigSurface::new(SurfaceKind::Mcp, sp, p.exists())
    }

    fn surface_skills(&self) -> Vec<ConfigSurface> {
        let base = if self.layout.skills_at_user_root {
            self.user_dir.join(self.layout.skills_dir)
        } else {
            self.project_dir.join(self.layout.skills_dir)
        };
        let prefix = if self.layout.skills_at_user_root {
            format!("{}{}", self.user_prefix(), self.layout.skills_dir)
        } else {
            format!("{}{}", self.dir_prefix(), self.layout.skills_dir)
        };
        Self::enumerate_skill_surfaces(&base, &prefix)
    }

    fn surface_agents(&self) -> Vec<ConfigSurface> {
        let base = if self.layout.agents_at_user_root {
            self.user_dir.join(self.layout.agents_dir)
        } else {
            self.project_dir.join(self.layout.agents_dir)
        };
        let prefix = if self.layout.agents_at_user_root {
            format!("{}{}", self.user_prefix(), self.layout.agents_dir)
        } else {
            format!("{}{}", self.dir_prefix(), self.layout.agents_dir)
        };
        let mut out = Vec::new();
        if base.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&base) {
                let mut found = false;
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_file() && p.extension().is_some_and(|e| e == "md") {
                        let name = p.file_name().unwrap().to_string_lossy().to_string();
                        out.push(ConfigSurface::new(
                            SurfaceKind::Agents,
                            format!("{prefix}/{name}"),
                            true,
                        ));
                        found = true;
                    }
                }
                if !found {
                    out.push(ConfigSurface::new(
                        SurfaceKind::Agents,
                        format!("{prefix}/"),
                        false,
                    ));
                }
            }
        } else {
            out.push(ConfigSurface::new(
                SurfaceKind::Agents,
                format!("{prefix}/"),
                false,
            ));
        }
        out
    }

    fn enumerate_skill_surfaces(base: &Path, prefix: &str) -> Vec<ConfigSurface> {
        let mut out = Vec::new();
        if base.is_dir() {
            if let Ok(entries) = std::fs::read_dir(base) {
                let mut found = false;
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_dir() && p.join("SKILL.md").exists() {
                        let name = p.file_name().unwrap().to_string_lossy().to_string();
                        out.push(ConfigSurface::new(
                            SurfaceKind::Skills,
                            format!("{prefix}/{name}/SKILL.md"),
                            true,
                        ));
                        found = true;
                    }
                }
                if !found {
                    out.push(ConfigSurface::new(
                        SurfaceKind::Skills,
                        format!("{prefix}/"),
                        false,
                    ));
                }
            }
        } else {
            out.push(ConfigSurface::new(
                SurfaceKind::Skills,
                format!("{prefix}/"),
                false,
            ));
        }
        out
    }

    /// 枚举"可能不存在"的配置目录型 surface（commands / hooks / memory）
    ///
    /// 探测 project 配置根或用户级配置根下的 `<dir_name>` 目录是否存在；
    /// 存在则 `present=true`（run_migration 据映射表决定迁移或报告 Unsupported），
    /// 不存在则 `present=false`（被 run_migration 跳过，不读不写）。
    ///
    /// 作用：使 `domestic_common_entries` 声明的 commands/hooks/memory 映射条目不再死代码，
    /// 能真实出现在扫描报告里；若源平台实际存在这些目录则被探测并迁移（前向兼容）。
    fn surface_optional_dir(&self, dir_name: &str, kind: SurfaceKind) -> ConfigSurface {
        let present =
            self.project_dir.join(dir_name).is_dir() || self.user_dir.join(dir_name).is_dir();
        let prefix = self.dir_prefix();
        ConfigSurface::new(kind, format!("{prefix}{dir_name}"), present)
    }
}

impl SourceAdapter for LayoutSource {
    fn name(&self) -> &'static str {
        self.layout.name
    }

    fn scan(&self) -> Vec<ConfigSurface> {
        let mut surfaces = Vec::new();
        surfaces.push(self.surface_instructions());
        surfaces.push(self.surface_mcp());
        surfaces.extend(self.surface_skills());
        surfaces.extend(self.surface_agents());
        // 完整 7 面：补齐 commands/hooks/memory，消除映射表死代码
        surfaces.push(self.surface_optional_dir(self.layout.commands_dir, SurfaceKind::Commands));
        surfaces.push(self.surface_optional_dir(self.layout.hooks_dir, SurfaceKind::Hooks));
        surfaces.push(self.surface_optional_dir(self.layout.memory_dir, SurfaceKind::Memory));
        surfaces
    }

    fn read(&self, surface: &ConfigSurface) -> Option<String> {
        // dsh（DeepSeek Harness）: MCP 是 cordis patch YAML 插件表，不是 JSON 文件。
        // 只解出 migrator 托管块并还原为标准 mcpServers JSON，
        // 块外用户私有条目视为「非本工具管理」，不作为迁移源。
        if surface.kind == SurfaceKind::Mcp
            && self.layout.mcp_format == McpFormat::CordisYamlPluginList
        {
            let path = self.mcp_config_path();
            let content = std::fs::read_to_string(&path).ok()?;
            let block = mcp_yaml::extract_cordis_mcp_block(&content)?;
            return mcp_yaml::cordis_block_to_json(&block).ok();
        }
        let path = self.path_from_source(&surface.source_path);
        if !path.exists() {
            return None;
        }
        if path.is_dir() {
            return Some(String::new());
        }
        let text = std::fs::read_to_string(path).ok()?;
        // instructions 是本工具唯一会加托管块的配置面。源文件里若残留
        // `<!-- BEGIN MIGRATOR:... -->`（说明它本身曾是迁移目标），先剥离标记
        // 再作为迁移源，否则 A→B→A 会块中块无限套娃（实测 dsh→codex 出现 4 个标记）。
        if surface.kind == SurfaceKind::Instructions {
            return Some(strip_markers(&text).0);
        }
        Some(text)
    }

    fn skill_support_dirs(&self, source_path: &str) -> Vec<PathBuf> {
        let path = self.path_from_source(source_path);
        let skill_root = match path.parent() {
            Some(p) => p.to_path_buf(),
            None => return Vec::new(),
        };
        super::collect_skill_subdirs(&skill_root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("migrator-lsrc-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_qoder_config_as_source() {
        let dir = temp_dir("qoder");
        let proj = dir.join("proj");
        let qoder = proj.join(".qoder");
        fs::create_dir_all(qoder.join("skills/s1")).unwrap();
        fs::write(proj.join("AGENTS.md"), "qoder rules").unwrap();
        fs::write(
            qoder.join(".mcp.json"),
            r#"{"mcpServers":{"q1":{"command":"npx"}}}"#,
        )
        .unwrap();
        fs::write(
            qoder.join("skills/s1/SKILL.md"),
            "---\nname: s1\n---\nSkill 1.",
        )
        .unwrap();

        let src = LayoutSource::detect(TargetLayout::qoder(), &proj);
        let surfaces = src.scan();
        // instructions 在项目根
        let instr = surfaces
            .iter()
            .find(|s| s.kind == SurfaceKind::Instructions)
            .unwrap();
        assert!(instr.present);
        assert!(instr.source_path.ends_with("AGENTS.md"));
        // mcp 在 .qoder/.mcp.json
        let mcp = surfaces
            .iter()
            .find(|s| s.kind == SurfaceKind::Mcp)
            .unwrap();
        assert!(mcp.present);
        assert!(mcp.source_path.contains(".qoder/.mcp.json"));
        // skills 枚举
        let skills: Vec<_> = surfaces
            .iter()
            .filter(|s| s.kind == SurfaceKind::Skills)
            .collect();
        assert_eq!(skills.len(), 1);
        assert!(src.read(skills[0]).unwrap().contains("Skill 1."));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reads_workbuddy_user_config_as_source() {
        let dir = temp_dir("wb");
        let home = dir.join("home");
        let wb = home.join(".workbuddy");
        fs::create_dir_all(wb.join("skills/s1")).unwrap();
        fs::write(wb.join("MEMORY.md"), "# User Memory\n").unwrap();
        fs::write(
            wb.join("mcp.json"),
            r#"{"mcpServers":{"w1":{"command":"npx"}}}"#,
        )
        .unwrap();
        fs::write(
            wb.join("skills/s1/SKILL.md"),
            "---\nname: s1\n---\nWB skill.",
        )
        .unwrap();

        // 直接构造（不依赖 HOME 环境变量）
        let src = LayoutSource::new(
            TargetLayout::workbuddy(),
            wb.clone(),
            dir.join("proj").join(".workbuddy"),
            dir.join("proj"),
        );
        let surfaces = src.scan();
        let instr = surfaces
            .iter()
            .find(|s| s.kind == SurfaceKind::Instructions)
            .unwrap();
        assert!(instr.present);
        assert!(instr.source_path.contains("MEMORY.md"));
        let mcp = surfaces
            .iter()
            .find(|s| s.kind == SurfaceKind::Mcp)
            .unwrap();
        assert!(mcp.present);
        assert!(mcp.source_path.contains("~/.workbuddy/mcp.json"));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// dsh（DeepSeek Harness）作为源: 用户级 AGENTS.md + profile 的 cordis patch YAML
    #[test]
    fn reads_dsh_user_config_as_source() {
        let dir = temp_dir("dsh");
        let dsh = dir.join("home").join(".dsh");
        let proj = dir.join("proj");
        fs::create_dir_all(dsh.join("profiles/desktop")).unwrap();
        fs::create_dir_all(dsh.join("skills/s1")).unwrap();
        fs::write(dsh.join("AGENTS.md"), "# dsh rules\n").unwrap();
        fs::write(
            dsh.join("skills/s1/SKILL.md"),
            "---\nname: s1\n---\ndsh skill.",
        )
        .unwrap();
        // 用户自己的 patch 条目 + 我们的托管块（后者才是迁移源）
        let patch = format!(
            "- id: llm-pi-ai\n  name: '@deepseek-ai/dsh-llm-pi-ai'\n  config:\n    providers: {{}}\n\
             {} \n- id: mcp-github\n  name: '@deepseek-ai/dsh-mcp-client'\n  config:\n    \
             serverName: 'github'\n    transport: streamable-http\n    url: 'https://api.githubcopilot.com/mcp/'\n    \
             headers:\n      Authorization: !!js '`Bearer ${{process.env.GITHUB_TOKEN}}`'\n{}\n",
            mcp_yaml::DSH_MCP_BEGIN,
            mcp_yaml::DSH_MCP_END
        );
        fs::write(dsh.join("profiles/desktop/cordis.patch.yml"), patch).unwrap();

        let src = LayoutSource::new(
            TargetLayout::dsh(),
            dsh.clone(),
            proj.join(".dsh"),
            proj.clone(),
        );
        let surfaces = src.scan();
        let instr = surfaces
            .iter()
            .find(|s| s.kind == SurfaceKind::Instructions)
            .unwrap();
        assert!(instr.present);
        assert_eq!(instr.source_path, "~/.dsh/AGENTS.md");
        let mcp = surfaces
            .iter()
            .find(|s| s.kind == SurfaceKind::Mcp)
            .unwrap();
        assert!(mcp.present);
        assert_eq!(mcp.source_path, "~/.dsh/profiles/desktop/cordis.patch.yml");
        // 读取必须产出标准 mcpServers JSON（含 !!js → {env:VAR} 还原）
        let json = src.read(mcp).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            v["mcpServers"]["github"]["url"],
            "https://api.githubcopilot.com/mcp/"
        );
        assert_eq!(
            v["mcpServers"]["github"]["headers"]["Authorization"],
            "Bearer {env:GITHUB_TOKEN}"
        );
        // skills 枚举
        let skills: Vec<_> = surfaces
            .iter()
            .filter(|s| s.kind == SurfaceKind::Skills)
            .collect();
        assert_eq!(skills.len(), 1);
        assert!(skills[0].source_path.starts_with("~/.dsh/skills/"));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// 豆包作为源: 读手动迁移包（粘贴件）
    #[test]
    fn reads_doubao_manual_bundle_as_source() {
        let dir = temp_dir("doubao");
        let proj = dir.join("proj");
        let manual = proj.join(".doubao/manual");
        fs::create_dir_all(&manual).unwrap();
        fs::write(manual.join("instructions.md"), "# 豆包规则\n").unwrap();
        fs::write(
            manual.join("connectors.json"),
            r#"{"mcpServers":{"ctx":{"command":"npx","args":["-y","@upstash/context7-mcp"]}}}"#,
        )
        .unwrap();

        let src = LayoutSource::new(
            TargetLayout::doubao(),
            dir.join("home").join(".doubao"),
            proj.join(".doubao"),
            proj.clone(),
        );
        let surfaces = src.scan();
        let instr = surfaces
            .iter()
            .find(|s| s.kind == SurfaceKind::Instructions)
            .unwrap();
        assert!(instr.present);
        assert!(instr.source_path.contains("instructions.md"));
        let mcp = surfaces
            .iter()
            .find(|s| s.kind == SurfaceKind::Mcp)
            .unwrap();
        assert!(mcp.present);
        assert!(mcp.source_path.ends_with(".doubao/manual/connectors.json"));
        assert!(src.read(mcp).unwrap().contains("context7"));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// #9 回归：逆向源扫描必须覆盖完整 7 面（instructions/mcp/skills/agents/commands/hooks/memory）
    #[test]
    fn scan_covers_all_seven_surfaces() {
        let dir = temp_dir("scan7");
        let proj = dir.join("proj");
        let qoder = proj.join(".qoder");
        fs::create_dir_all(&qoder).unwrap();
        fs::write(proj.join("AGENTS.md"), "rules").unwrap();
        fs::write(qoder.join(".mcp.json"), r#"{"mcpServers":{}}"#).unwrap();

        // 直接构造并用受控临时 user_dir，隔离真实 HOME（避免探测到机器上已装的 qoder 目录）
        let home = dir.join("home");
        fs::create_dir_all(&home).unwrap();
        let src = LayoutSource::new(
            TargetLayout::qoder(),
            home.clone(),
            qoder.clone(),
            proj.clone(),
        );
        let surfaces = src.scan();
        let kinds: Vec<SurfaceKind> = surfaces.iter().map(|s| s.kind).collect();
        for expected in [
            SurfaceKind::Instructions,
            SurfaceKind::Mcp,
            SurfaceKind::Skills,
            SurfaceKind::Agents,
            SurfaceKind::Commands,
            SurfaceKind::Hooks,
            SurfaceKind::Memory,
        ] {
            assert!(
                kinds.contains(&expected),
                "scan 漏掉 surface: {}",
                expected.as_str()
            );
        }
        // 国内平台无 commands/hooks/memory 对应物 → 这些面 present=false（被 run_migration 跳过）
        let cmd = surfaces
            .iter()
            .find(|s| s.kind == SurfaceKind::Commands)
            .unwrap();
        assert!(!cmd.present, "qoder 无 commands 目录，应 present=false");
        let mem = surfaces
            .iter()
            .find(|s| s.kind == SurfaceKind::Memory)
            .unwrap();
        assert!(!mem.present, "qoder 无 memory 目录，应 present=false");
        fs::remove_dir_all(&dir).unwrap();
    }
}
