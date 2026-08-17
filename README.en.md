# migrator — Multi-Target Agent Config Migrator

> One-command migration of Claude Code / Codex configs to Chinese AI coding platforms (Qoder / Trae / Lingma / MarsCode...)
> Platform-neutral · Safety-first · Community extensible

[![Rust](https://img.shields.io/badge/Rust-1.97+-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/javaAndyhy/migrator)](https://github.com/javaAndyhy/migrator/releases)

[中文版](README.md) | **English**

---

## Why

In 2026, migrating between AI coding tools became a daily reality: security controversies around Claude Code, Alibaba banning Claude entirely, OpenAI launching Codex `/import` to poach users — developers everywhere are moving from Claude Code / Codex to domestic platforms.

But existing migration tools are all **single-target official migrations** (each platform writes its own mover), and most lack safety mechanisms:

| Tool | Source → Target | Safety |
|---|---|---|
| OpenAI Codex `/import` | Claude/Cursor → Codex | ❌ No rollback |
| Alibaba Qoder migration skill | Claude → Qoder | ⚠️ Copy-only, no backup |
| **migrator (this project)** | **Claude / Codex → any platform** | ✅ Managed blocks + backup/restore + atomic write + idempotent |

**migrator differentiator**: platform-neutral + a complete safety protocol (absorbed from community best practices).

---

## Quick Start

```bash
# Scan source configs (read-only)
migrator --source codex scan --project .

# Generate migration plan (read-only)
migrator --source codex plan --project .

# Dry run (read-only, nothing written)
migrator --source codex dry-run --project .

# Execute migration (requires explicit --yes; read-only by default)
# Targets: qoder | trae | lingma (default qoder)
migrator --source codex --target qoder apply --project . --yes
migrator --source claude --target trae apply --project . --yes
migrator --source codex --target lingma apply --project . --yes

# Validate target
migrator --source codex validate --project .
```

### Target Platform Differences

| Platform | Instructions | MCP config | Config root |
|---|---|---|---|
| Qoder | `AGENTS.md` | `.mcp.json` | `.qoder/` |
| Trae | `AGENTS.md` | `mcp.json` | `.trae/` |
| Lingma (Alibaba) | `LINGMA.md` | `mcp-settings.json` | `.lingma/` |

Platform differences live entirely in the *output layout* — add a new platform by defining a layout in `src/target/layout.rs` plus a mapping JSON.

### Platform-Specific Mapping Tables

Mappings are maintained per `source × target` pair (`data/mappings/<source>-to-<target>.json`), so each pair can carry its own semantic judgments (e.g. Trae has no sub-agents → `unsupported`; Lingma's mcp-settings.json schema differs → `partial`):

```text
data/mappings/
├── claude-to-qoder.json    ← Claude Code → Qoder
├── claude-to-trae.json     ← Claude Code → Trae (agents: unsupported)
├── claude-to-lingma.json   ← Claude Code → Lingma (mcp: partial)
├── codex-to-qoder.json     ← Codex → Qoder (mcp: TOML→JSON)
├── codex-to-trae.json      ← Codex → Trae (agents: unsupported)
└── codex-to-lingma.json    ← Codex → Lingma (mcp: partial)
```

Resolution order: explicit `--mapping` flag → platform default file → builtin table (`MappingTable::builtin`). Adding a platform pair = adding one JSON file, zero Rust changes.

### Safety Rollback Chain

```bash
# List backup batches
migrator backups --project .

# Restore (dry run by default; --yes actually rolls back)
migrator restore --project . --yes

# Clean up backups (user files untouched)
migrator clean --project . --yes
```

---

## Architecture

```
Source Adapter → Converter (mapping-driven) → Target Adapter → Safety Write Layer
      ↓                 ↓                     ↓
   scan/read        semantic downgrade     generate/write
      └────────────→ Safety Write Layer ←──────────┘
                (managed block · atomic · backup)
                        ↓
                Report (3-state + 3-level scope)
```

**Four load-bearing layers** (keel architecture review):
1. **Source Adapter** — reads source platform configs (read-only contract): `claude` / `codex`
2. **Converter** — mapping-table driven, semantic downgrade → `MANUAL MIGRATION REQUIRED` block
3. **Target Adapter** — generates target platform formats: `qoder` / `trae` / `lingma`
4. **Safety Write Layer** — managed block v1, atomic write, backup/restore/clean, idempotent

### Five Safety Contracts (v1 frozen)

| Contract | Implementation |
|---|---|
| Read-only by default | `apply` requires explicit `--yes` |
| Managed blocks | `<!-- BEGIN/END MIGRATOR:<scope> -->`, preserves handwritten content, rejects malformed markers |
| Atomic write | `.tmp` + rename, no partial writes on interruption |
| Backup & rollback | auto-backup before write → `restore` (dry run) → `clean` |
| Idempotent | re-running the same migration produces identical results |

---

## Supported Surfaces

| Surface | Claude Code source | Codex source | Conversion |
|---|---|---|---|
| instructions | CLAUDE.md | AGENTS.md | → AGENTS.md (managed block) |
| mcp | .mcp.json | config.toml [mcp_servers] | → .mcp.json (TOML→JSON, nested env merged) |
| skills | .claude/skills/ | ~/.codex/skills/ | frontmatter downgrade + support dirs copied |
| agents | .claude/agents/*.md | ~/.codex/agents/*.toml | YAML/TOML → target format |
| memory | ~/.claude/memory/ | ~/.codex/memories/ | **read-only index** (content stays in source) |

### Three-State Semantic Report

```
[Added               ] skills xxx — convert
[Check before using  ] skills yyy — fields downgraded: model, paths
[Not Added           ] hooks     — no target equivalent
```

Partial conversions embed a `## MANUAL MIGRATION REQUIRED` block in the target file with review guidance.

---

## Community Extension

**New target platform = write one JSON mapping file** (no Rust changes):

```json
// data/mappings/claude-to-qoder.json
{
  "schema_version": 1,
  "source": "claude-code",
  "target": "qoder",
  "entries": [
    { "source": "instructions", "target": "AGENTS.md", "behavior": "convert", "status": "exact" }
  ]
}
```

```bash
migrator --mapping data/mappings/claude-to-qoder.json apply --project . --yes
```

---

## Design Origins (Absorbed & Distilled)

Architecture absorbed from the 2026 AI-coding migration tool ecosystem:

- **migrate-to-codex** (OpenAI official): mapping-table driven, three-state report, MANUAL block
- **claude-codex-sync** (RuntianLee): managed blocks, read-only memory index, backup/rollback chain
- **claude-to-qoder-migration** (Alibaba official): three-level scope, copy-only
- **our-codex** (Getty): frontmatter stripping, hierarchy sorting

Then corrected via **keel** (load-bearing architecture review): report demoted to output format, contract mechanisms (managed block v1 freeze / mapping schema versioning / adapter contract tests), negative paths (atomic write / idempotency / race detection).

---

## Development

```bash
# Note: in sandboxed environments the target dir is one-shot; use cargo.sh to auto-allocate a fresh one
./cargo.sh test
./cargo.sh build

# Direct build (default target in normal environments)
cargo build
```

Tests: **70 unit tests** (managed block / atomic write / backup rollback / mapping tables / converters / dual-source adapters), zero warnings.

---

## Roadmap

- [x] Phase 1: four-layer pipeline skeleton + managed block v1 + atomic write + mapping schema v1
- [x] Phase 2: semantic downgrade + MANUAL block / backup rollback chain / JSON mappings / read-only memory index
- [x] Real-world validation: Codex source (45 skills + 9 agents + 10 MCP servers end-to-end)
- [x] Phase 3: multi-target adapters (Qoder / Trae / Lingma) + platform-specific mappings
- [ ] Reverse migration (domestic → Claude/Codex)
- [ ] Session history migration
- [ ] GitHub Actions CI (multi-platform release builds)

## License

MIT
