//! Codex 源适配器 — 读取 ~/.codex 配置
//!
//! 只读: 本模块绝不修改源文件（契约）
//! 格式: AGENTS.md / config.toml (mcp_servers) / skills/<name>/SKILL.md / agents/*.toml / memories/*.md

use super::SourceAdapter;
use crate::model::{ConfigSurface, SurfaceKind};
use std::path::PathBuf;

/// Codex 源适配器
pub struct CodexSource {
    /// 用户级配置目录 (~/.codex)
    user_dir: PathBuf,
}

impl CodexSource {
    pub fn new(user_dir: impl Into<PathBuf>) -> Self {
        Self {
            user_dir: user_dir.into(),
        }
    }

    /// 从当前用户 HOME 探测 ~/.codex
    pub fn detect() -> Self {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".into());
        Self::new(PathBuf::from(home).join(".codex"))
    }
}

impl SourceAdapter for CodexSource {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn scan(&self) -> Vec<ConfigSurface> {
        let mut surfaces = Vec::new();

        // instructions: AGENTS.md
        let agents_md = self.user_dir.join("AGENTS.md");
        surfaces.push(ConfigSurface::new(
            SurfaceKind::Instructions,
            "~/.codex/AGENTS.md",
            agents_md.exists(),
        ));

        // mcp: config.toml 中的 mcp_servers 段
        let config = self.user_dir.join("config.toml");
        surfaces.push(ConfigSurface::new(
            SurfaceKind::Mcp,
            "~/.codex/config.toml ([mcp_servers])",
            config.exists(),
        ));

        // skills: 枚举 ~/.codex/skills/<name>/SKILL.md
        let skills_dir = self.user_dir.join("skills");
        if skills_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&skills_dir) {
                let mut found_any = false;
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_dir() && p.join("SKILL.md").exists() {
                        let name = p.file_name().unwrap().to_string_lossy().to_string();
                        surfaces.push(ConfigSurface::new(
                            SurfaceKind::Skills,
                            format!("~/.codex/skills/{name}/SKILL.md"),
                            true,
                        ));
                        found_any = true;
                    }
                }
                if !found_any {
                    surfaces.push(ConfigSurface::new(
                        SurfaceKind::Skills,
                        "~/.codex/skills/",
                        false,
                    ));
                }
            }
        } else {
            surfaces.push(ConfigSurface::new(
                SurfaceKind::Skills,
                "~/.codex/skills/",
                false,
            ));
        }

        // agents: 枚举 ~/.codex/agents/*.toml
        let agents_dir = self.user_dir.join("agents");
        if agents_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&agents_dir) {
                let mut found_any = false;
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_file() && p.extension().is_some_and(|e| e == "toml") {
                        let name = p.file_name().unwrap().to_string_lossy().to_string();
                        surfaces.push(ConfigSurface::new(
                            SurfaceKind::Agents,
                            format!("~/.codex/agents/{name}"),
                            true,
                        ));
                        found_any = true;
                    }
                }
                if !found_any {
                    surfaces.push(ConfigSurface::new(
                        SurfaceKind::Agents,
                        "~/.codex/agents/",
                        false,
                    ));
                }
            }
        } else {
            surfaces.push(ConfigSurface::new(
                SurfaceKind::Agents,
                "~/.codex/agents/",
                false,
            ));
        }

        // memory: ~/.codex/memories/*.md
        let memories_dir = self.user_dir.join("memories");
        let has_memory = memories_dir.is_dir()
            && std::fs::read_dir(&memories_dir)
                .map(|it| it.flatten().any(|e| e.path().is_file()))
                .unwrap_or(false);
        surfaces.push(ConfigSurface::new(
            SurfaceKind::Memory,
            "~/.codex/memories/",
            has_memory,
        ));

        surfaces
    }

    fn read(&self, surface: &ConfigSurface) -> Option<String> {
        let path = match surface.kind {
            SurfaceKind::Instructions => self.user_dir.join("AGENTS.md"),
            SurfaceKind::Mcp => self.user_dir.join("config.toml"),
            SurfaceKind::Skills => {
                // source_path 形如 ~/.codex/skills/<name>/SKILL.md
                let rel = surface.source_path.trim_start_matches("~/.codex/");
                self.user_dir.join(rel)
            }
            SurfaceKind::Agents => {
                let rel = surface.source_path.trim_start_matches("~/.codex/");
                self.user_dir.join(rel)
            }
            SurfaceKind::Memory => self.user_dir.join("memories"),
            _ => return None,
        };
        if !path.exists() {
            return None;
        }
        // Memory: 返回目录内 .md 文件清单（每行一个绝对路径）
        if surface.kind == SurfaceKind::Memory && path.is_dir() {
            let files: Vec<String> = std::fs::read_dir(&path)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| {
                    e.path().is_file()
                        && e.path()
                            .extension()
                            .is_some_and(|x| x == "md" || x == "markdown")
                })
                .map(|e| e.path().to_string_lossy().to_string())
                .collect();
            if files.is_empty() {
                return None;
            }
            return Some(files.join("\n"));
        }
        if path.is_dir() {
            return Some(String::new());
        }
        std::fs::read_to_string(path).ok()
    }

    fn skill_support_dirs(&self, source_path: &str) -> Vec<PathBuf> {
        // source_path 形如 ~/.codex/skills/<name>/SKILL.md
        let rel = source_path.trim_start_matches("~/.codex/");
        let skill_dir = self.user_dir.join(rel);
        let skill_root = match skill_dir.parent() {
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
        let dir = std::env::temp_dir().join(format!("migrator-codex-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scan_detects_codex_surfaces() {
        let dir = temp_dir("scan");
        let codex = dir.join(".codex");
        fs::create_dir_all(&codex).unwrap();
        fs::write(codex.join("AGENTS.md"), "# Rules").unwrap();
        fs::write(codex.join("config.toml"), "model = \"gpt-5.6\"\n").unwrap();

        let src = CodexSource::new(&codex);
        let surfaces = src.scan();
        let instructions = surfaces
            .iter()
            .find(|s| s.kind == SurfaceKind::Instructions)
            .unwrap();
        assert!(instructions.present);
        let mcp = surfaces.iter().find(|s| s.kind == SurfaceKind::Mcp).unwrap();
        assert!(mcp.present);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn scan_enumerates_agents_toml() {
        let dir = temp_dir("agents");
        let codex = dir.join(".codex");
        fs::create_dir_all(codex.join("agents")).unwrap();
        fs::write(codex.join("agents/mtkf-coder.toml"), "name = \"coder\"\n").unwrap();
        fs::write(codex.join("agents/notes.txt"), "not an agent\n").unwrap();

        let src = CodexSource::new(&codex);
        let surfaces = src.scan();
        let agents: Vec<&ConfigSurface> = surfaces
            .iter()
            .filter(|s| s.kind == SurfaceKind::Agents)
            .collect();
        assert_eq!(agents.len(), 1);
        assert!(agents[0].source_path.contains("mtkf-coder.toml"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn read_agent_toml() {
        let dir = temp_dir("read");
        let codex = dir.join(".codex");
        fs::create_dir_all(codex.join("agents")).unwrap();
        fs::write(codex.join("agents/a.toml"), "name = \"a\"\n").unwrap();

        let src = CodexSource::new(&codex);
        let surface = ConfigSurface::new(SurfaceKind::Agents, "~/.codex/agents/a.toml", true);
        let content = src.read(&surface).unwrap();
        assert_eq!(content, "name = \"a\"\n");
        fs::remove_dir_all(&dir).unwrap();
    }
}
