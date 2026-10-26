# migrator — Multi-Target Agent Config Migrator

> One-command, **bidirectional** migration of Claude Code / Codex configs to and from Chinese AI coding platforms (Qoder / Trae / Lingma (now Qoder CN) / WorkBuddy / DeepSeek Harness (dsh) / Doubao / MarsCode...).
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
# Targets: qoder | trae | lingma (now Qoder CN) | workbuddy | dsh | doubao (default qoder)
migrator --source codex --target qoder apply --project . --yes
migrator --source claude --target trae apply --project . --yes
migrator --source codex --target lingma apply --project . --yes
migrator --source codex --target workbuddy apply --project . --yes
migrator --source codex --target dsh apply --project . --yes       # DeepSeek Harness
migrator --source claude --target doubao apply --project . --yes   # Doubao (paste bundle)

# Migrate a single surface only (skips MCP so clients don't reload connectors)
# Surfaces: instructions | mcp | skills | agents | commands | hooks | memory
migrator --source codex --target workbuddy --only instructions apply --project . --yes

# Validate target
migrator --source codex validate --project .
```

### Target Platform Differences

| Platform | Instructions | MCP config | Config root |
|---|---|---|---|
| Qoder | `AGENTS.md` (project root) | `.mcp.json` (merged) | `.qoder/` |
| Trae | `AGENTS.md` (project root) | `mcp.json` (merged) | `.trae/` |
| Lingma (renamed Qoder CN) | `AGENTS.md` (project root; IDE rules in `.lingma/rules/`) | `mcp-settings.json` (partial) | `.lingma/` |
| WorkBuddy | `MEMORY.md` (managed reference) | `mcp.json` (user-level `~/.workbuddy/`, merged) | `.workbuddy/` |
| **DeepSeek Harness (dsh)** | `~/.dsh/AGENTS.md` (user-level) | **`~/.dsh/profiles/<profile>/cordis.patch.yml`** (cordis patch YAML plugin entries, *not* JSON) | `.dsh/` |
| **Doubao (Doubao desktop)** | paste bundle `.doubao/manual/instructions.md` | paste bundle `.doubao/manual/connectors.json` (client has no file-level config contract) | `.doubao/` |

> **Platform evolution facts** (verified 2026-08-18 via context7 / official docs):
> - **Qoder**: rules are compatible with AGENTS.md placed at the **project root** (docs.qoder.com/user-guide/rules); Custom Agents are `.md` + YAML frontmatter (name/description/tools/model/skills/mcpServers).
> - **Lingma → Qoder CN**: Alibaba folded Lingma into the Qoder CN family (IDE / JetBrains / VS Code plugins). Rules now live in `.lingma/rules/` (IDE) plus a three-tier AGENTS.md (CLI: user `~/.qoder/AGENTS.md` / project / local); `LINGMA.md` is deprecated. Agents live in `.lingma/agents/*.md`.
> - **Trae**: brand evolved into TraeWork (docs.trae.ai); this tool's Trae layout is based on earlier official info (AGENTS.md + `.trae/mcp.json`) and needs re-verification.

> **WorkBuddy specifics** (verified 2026-08-17): instructions are copied in full to `~/.workbuddy/rules/`, with `MEMORY.md` keeping only a short managed reference (so hundreds of rule lines never blow the memory size limit); MCP is merged into the official user-level `~/.workbuddy/mcp.json` preserving existing servers; agents / memory have no WorkBuddy equivalent → `unsupported`.

> **DeepSeek Harness (dsh) specifics** (verified on a real client, 2026-10-26, cross-checked against the `@deepseek-ai/dsh-*` package READMEs):
> - instructions: user-level `$DSH_HOME/AGENTS.md` (default `~/.dsh/AGENTS.md`). dsh loads it first, then walks project `AGENTS.md` / `CLAUDE.md` (+ `*.local.md` overlays) up from projectRoot → cwd. Migration writes the user-level file, matching Codex's `~/.codex/AGENTS.md` global-rule semantics.
> - MCP: **not** `mcp.json` — it is a cordis patch list in `~/.dsh/profiles/<profile>/cordis.patch.yml`, with entries like `name: '@deepseek-ai/dsh-mcp-client'` and `config.serverName/transport/command/args/env/cwd/url/headers`. Writing uses a **YAML comment managed block** (`# <!-- BEGIN MIGRATOR:dsh:mcp -->`); entries outside the block, user comments and `!!js` expressions are preserved byte-for-byte (parse-then-reserialize would destroy them). `{env:VAR}` placeholders become `!!js process.env.VAR` and are restored on the way back.
> - skills: `~/.dsh/skills` (user-dsh root; project-level `.dsh/skills` also scanned). agents/hooks/memory: dsh sub-agents are preset/profile driven with no file-level `agents/*.md` → `unsupported`.

> **Doubao specifics** (verified on a real client, 2026-10-26): Doubao desktop has **no file-level config contract** — MCP "custom connectors", skills and rules all live in the client's internal store (`%LOCALAPPDATA%\Doubao\User Data\Default\Local Storage\leveldb`) and can only be pasted through the UI. The tool therefore emits a **paste bundle** at `{project}/.doubao/manual/` (`connectors.json` / `instructions.md` / `skills/` / `agents/` / a generated `README.md` with step-by-step instructions) and states explicitly that the target application does not read these files.

### Verified on three real domestic targets (2026-10-26)

Same `~/.codex` source, all three targets actually executed against installed desktop clients (not just unit tests):

| Target | Artifacts produced | Assertions |
|---|---|---|
| `dsh` | `~/.dsh/AGENTS.md`, `~/.dsh/profiles/desktop/cordis.patch.yml`, `~/.dsh/skills/` × 59 | valid YAML; 19 entries (6 pre-existing user entries + 13 MCP), unique ids; re-running `apply` leaves the file byte-identical |
| `workbuddy` | `~/.workbuddy/mcp.json`, `MEMORY.md` (394-byte managed reference), `rules/codex-rules.md`, `skills/` × 59 | `mcp.json` is valid JSON with 13 servers |
| `doubao` | `.doubao/manual/{README.md, connectors.json, instructions.md, agents × 15, skills × 59}` | `connectors.json` is valid JSON with 13 servers |

Reverse direction was verified too: `--source dsh --target codex` reads the cordis managed block back and produces a valid `config.toml` restoring all 13 servers. Commands, verification scripts and the three defects these runs exposed are documented in [docs/REAL_MIGRATION.md](docs/REAL_MIGRATION.md) §8–§12.

Platform differences live entirely in the *output layout* — add a new platform by defining a layout in `src/target/layout.rs` plus a mapping JSON.

### Platform-Specific Mapping Tables

Mappings are maintained per `source × target` pair (`data/mappings/<source>-to-<target>.json`), so each pair can carry its own semantic judgments (e.g. Trae has no sub-agents → `unsupported`; Lingma's (Qoder CN) mcp-settings.json schema differs → `partial`):

```text
data/mappings/
├── claude-to-qoder.json      ← Claude Code → Qoder (mcp: merge)
├── claude-to-trae.json       ← Claude Code → Trae (agents: unsupported; mcp: merge)
├── claude-to-lingma.json     ← Claude Code → Qoder CN Lingma (instructions: AGENTS.md; mcp: partial)
├── claude-to-workbuddy.json  ← Claude Code → WorkBuddy (mcp: user-level merge; agents/memory: unsupported)
├── claude-to-dsh.json        ← Claude Code → DeepSeek Harness (mcp: cordis patch YAML; agents: unsupported)
├── claude-to-doubao.json     ← Claude Code → Doubao (everything is a manual paste bundle)
├── codex-to-qoder.json       ← Codex → Qoder (mcp: TOML→JSON merge)
├── codex-to-trae.json        ← Codex → Trae (agents: unsupported; mcp: merge)
├── codex-to-lingma.json      ← Codex → Qoder CN Lingma (instructions: AGENTS.md; mcp: partial)
└── codex-to-workbuddy.json   ← Codex → WorkBuddy (mcp: user-level merge; agents/memory: unsupported)
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
3. **Target Adapter** — generates target platform formats: `qoder` / `trae` / `lingma` / `workbuddy` / `dsh` (cordis YAML) / `doubao` (paste bundle) / `claude` / `codex`
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
| instructions | CLAUDE.md | AGENTS.md | → target rules (Qoder/Trae/Lingma (now Qoder CN): project-root AGENTS.md; WorkBuddy: rules/ reference) |
| mcp | .mcp.json | config.toml [mcp_servers] | → target MCP file (TOML→JSON; qoder/trae/workbuddy merge preserving existing) |
| skills | .claude/skills/ | ~/.codex/skills/ | frontmatter downgrade + support dirs copied |
| agents | .claude/agents/*.md | ~/.codex/agents/*.toml | YAML/TOML → Qoder/Lingma (now Qoder CN) .md; Trae/WorkBuddy unsupported |
| commands | .claude/commands/*.md | — | → single-file skill (`skills/<name>/SKILL.md`, convert) |
| hooks | .claude/hooks/hooks.json | ~/.codex/hooks.json | scan & report only (no target equivalent → Not Added) |
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

Tests: **83 unit tests** (managed block / atomic write / backup rollback / mapping tables / converters / dual-source adapters / hooks·commands), zero warnings.

---

## Roadmap

- [x] Phase 1: four-layer pipeline skeleton + managed block v1 + atomic write + mapping schema v1
- [x] Phase 2: semantic downgrade + MANUAL block / backup rollback chain / JSON mappings / read-only memory index
- [x] Real-world validation: Codex source (45 skills + 15 agents + 10 MCP servers end-to-end)
- [x] Phase 3: multi-target adapters (Qoder / Trae / Lingma (now Qoder CN)) + platform-specific mappings
- [x] Phase 4: WorkBuddy target (user-level MCP merge / rules-reference / agents·memory downgrade)
- [x] Phase 5: commands→skills conversion + hooks scan & report + target path in reports
- [ ] Reverse migration (domestic → Claude/Codex)
- [ ] Session history migration
- [x] GitHub Actions CI (3-OS test + release builds)

## License

MIT
