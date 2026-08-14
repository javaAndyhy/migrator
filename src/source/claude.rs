//! Claude Code 源适配器 — 读取 ~/.claude 和项目 .claude 配置
//!
//! 只读: 本模块绝不修改源文件（契约）

use super::SourceAdapter;
use crate::model::ConfigSurface;
use crate::model::SurfaceKind;
use std::path::{Path, PathBuf};

/// Claude Code 源适配器
pub struct ClaudeCodeSource {
    /// 用户级配置目录 (~/.claude)
    user_dir: PathBuf,
    /// 项目级配置目录 (./.claude)
    project_dir: PathBuf,
}

impl ClaudeCodeSource {
    pub fn new(user_dir: impl Into<PathBuf>, project_dir: impl Into<PathBuf>) -> Self {
        Self {
            user_dir: user_dir.into(),
            project_dir: project_dir.into(),
        }
    }

    /// 从当前目录探测 .claude 位置
    pub fn detect(current_dir: &Path) -> Self {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".into());
        Self::new(
            PathBuf::from(home).join(".claude"),
            current_dir.join(".claude"),
        )
    }
}

impl SourceAdapter for ClaudeCodeSource {
    fn name(&self) -> &'static str {
        "claude-code"
    }

    fn scan(&self) -> Vec<ConfigSurface> {
        let mut surfaces = Vec::new();

        // instructions: CLAUDE.md
        let claude_md = self.project_dir.join("CLAUDE.md");
        surfaces.push(ConfigSurface::new(
            SurfaceKind::Instructions,
            ".claude/CLAUDE.md",
            claude_md.exists(),
        ));

        // mcp: .mcp.json（项目级）
        let mcp_json = self.project_dir.join(".mcp.json");
        surfaces.push(ConfigSurface::new(
            SurfaceKind::Mcp,
            ".claude/.mcp.json",
            mcp_json.exists(),
        ));

        // mcp: settings.json 中的 mcpServers（用户级）
        let settings = self.user_dir.join("settings.json");
        surfaces.push(ConfigSurface::new(
            SurfaceKind::Mcp,
            "~/.claude/settings.json (mcpServers)",
            settings.exists(),
        ));

        // skills 目录 — 枚举每个 skill
        let skills_dir = self.project_dir.join("skills");
        if skills_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&skills_dir) {
                let mut found_any = false;
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_dir() && p.join("SKILL.md").exists() {
                        let name = p.file_name().unwrap().to_string_lossy().to_string();
                        surfaces.push(ConfigSurface::new(
                            SurfaceKind::Skills,
                            format!(".claude/skills/{name}/SKILL.md"),
                            true,
                        ));
                        found_any = true;
                    }
                }
                if !found_any {
                    surfaces.push(ConfigSurface::new(
                        SurfaceKind::Skills,
                        ".claude/skills/",
                        false,
                    ));
                }
            }
        } else {
            surfaces.push(ConfigSurface::new(
                SurfaceKind::Skills,
                ".claude/skills/",
                false,
            ));
        }

        // agents 目录 — 枚举每个 agent
        let agents_dir = self.project_dir.join("agents");
        if agents_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&agents_dir) {
                let mut found_any = false;
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_file() && p.extension().is_some_and(|e| e == "md") {
                        let name = p.file_name().unwrap().to_string_lossy().to_string();
                        surfaces.push(ConfigSurface::new(
                            SurfaceKind::Agents,
                            format!(".claude/agents/{name}"),
                            true,
                        ));
                        found_any = true;
                    }
                }
                if !found_any {
                    surfaces.push(ConfigSurface::new(
                        SurfaceKind::Agents,
                        ".claude/agents/",
                        false,
                    ));
                }
            }
        } else {
            surfaces.push(ConfigSurface::new(
                SurfaceKind::Agents,
                ".claude/agents/",
                false,
            ));
        }

        // memory: 用户级 ~/.claude/memory/ 目录（存在任意 .md 文件即标记）
        let memory_dir = self.user_dir.join("memory");
        let has_memory = memory_dir.is_dir()
            && std::fs::read_dir(&memory_dir)
                .map(|it| it.flatten().any(|e| e.path().is_file()))
                .unwrap_or(false);
        surfaces.push(ConfigSurface::new(
            SurfaceKind::Memory,
            "~/.claude/memory/",
            has_memory,
        ));

        surfaces
    }

    fn read(&self, surface: &ConfigSurface) -> Option<String> {
        let path = match surface.kind {
            SurfaceKind::Instructions => self.project_dir.join("CLAUDE.md"),
            SurfaceKind::Mcp => {
                if surface.source_path.starts_with("~/.claude/settings.json") {
                    self.user_dir.join("settings.json")
                } else {
                    self.project_dir.join(".mcp.json")
                }
            }
            SurfaceKind::Skills => {
                // source_path 形如 .claude/skills/<name>/SKILL.md
                let rel = surface.source_path.trim_start_matches(".claude/");
                self.project_dir.join(rel)
            }
            SurfaceKind::Agents => {
                let rel = surface.source_path.trim_start_matches(".claude/");
                self.project_dir.join(rel)
            }
            SurfaceKind::Memory => self.user_dir.join("memory"),
            _ => return None,
        };
        if !path.exists() {
            return None;
        }
        // Memory: 返回目录内 .md 文件清单（每行一个绝对路径），供索引生成器使用
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
        // 目录返回空（不应发生，scan 已枚举文件）
        if path.is_dir() {
            return Some(String::new());
        }
        std::fs::read_to_string(path).ok()
    }

    fn skill_support_dirs(&self, source_path: &str) -> Vec<PathBuf> {
        // source_path 形如 .claude/skills/<name>/SKILL.md
        let rel = source_path.trim_start_matches(".claude/");
        let skill_dir = self.project_dir.join(rel);
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
        let dir = std::env::temp_dir().join(format!("migrator-src-{}-{}", name, std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scan_detects_present_surfaces() {
        let dir = temp_dir("scan");
        let claude_dir = dir.join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();
        fs::write(claude_dir.join("CLAUDE.md"), "# Rules").unwrap();

        let src = ClaudeCodeSource::new(dir.join("home").join(".claude"), claude_dir);
        let surfaces = src.scan();
        let instructions = surfaces
            .iter()
            .find(|s| s.kind == SurfaceKind::Instructions)
            .unwrap();
        assert!(instructions.present);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn scan_marks_absent_surfaces() {
        let dir = temp_dir("absent");
        let src = ClaudeCodeSource::new(dir.join("home").join(".claude"), dir.join(".claude"));
        let surfaces = src.scan();
        assert!(surfaces.iter().all(|s| !s.present));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn read_instructions_content() {
        let dir = temp_dir("read");
        let claude_dir = dir.join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();
        fs::write(claude_dir.join("CLAUDE.md"), "always use Chinese").unwrap();

        let src = ClaudeCodeSource::new(dir.join("home").join(".claude"), claude_dir);
        let surface = ConfigSurface::new(SurfaceKind::Instructions, ".claude/CLAUDE.md", true);
        let content = src.read(&surface).unwrap();
        assert_eq!(content, "always use Chinese");

        fs::remove_dir_all(&dir).unwrap();
    }
}
