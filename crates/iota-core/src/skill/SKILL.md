---
name: iota-src-skill
description: Use when working on skill registry loading, trigger matching, advisory or MCP skill execution, skill cache, sandbox executor, or files under crates/iota-core/src/skill.
triggers:
  - crates/iota-core/src/skill
  - SkillRegistry
  - SkillExecution
  - SkillExecutionMode
  - fun
  - iota-fun
  - skill pull
---

# skill — Skill Layer

Loads `.md`/`.yaml` skill manifests, matches triggers against prompts, and executes skills in advisory or MCP mode.

## Responsibilities

- Load skill manifests from configured roots (local or HTTP)
- Match prompt text against skill trigger patterns
- Execute skills via advisory metadata or MCP (7-language sandbox executor)
- Cache remote skills locally

## Sub-modules

| Module | Purpose |
| :--------| :---------|
| `cache` | HTTP/local skill fetching and caching |
| `runner` | MCP-mode skill execution and template rendering |
| `fun` | `iota-fun` MCP server — 7-language execution (C++, Go, Java, Python, Rust, TypeScript, Zig) |

## Key Types

- `SkillRegistry` — loaded skill collection with trigger matching
- `Skill` — single skill definition with metadata and execution config
- `SkillMetadata` — name, description, triggers, tags
- `SkillExecution` — execution mode and parameters
- `SkillExecutionMode` — Advisory or Mcp
- `SkillCache` — local cache for pulled skills
