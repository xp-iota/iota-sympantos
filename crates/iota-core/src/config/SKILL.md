---
name: iota-src-config
description: Use when working on nimia.yaml loading, backend environment mapping, model provider config, context budgets, path expansion, or files under crates/iota-core/src/config.
triggers:
  - crates/iota-core/src/config
  - nimia.yaml
  - NimiaConfig
  - BackendConfig
  - EffectiveConfig
  - ContextEngineConfig
  - normalize_command
---

# config — Configuration

Parses `~/.i6/nimia.yaml` and provides typed configuration for all modules.

## Responsibilities

- Load and deserialize YAML config with serde
- Map model provider settings to backend-specific environment variables
- Provide per-backend context options (injection mode, budgets, thresholds)
- Resolve paths with home directory expansion
- Platform-aware command normalization (`npx` → `npx.cmd` on Windows)

## Sub-modules

| Module | Purpose |
| :--------| :---------|
| `adapters` | `BackendAdapter` strategy registry for mapping environments/commands |
| `backend` | `BackendConfig`, env variable mapping, command resolution |
| `helpers` | `expand_home_path()`, `normalize_command()` |
| `context` | `ContextEngineConfig`, `RecallThresholdsConfig`, `ContextBudgetsConfig`, `EmbeddingConfig` |
| `effective` | `EffectiveConfig` — resolved config with defaults applied |
| `loader` | `read_config()`, `config_path()` |
| `model` | `ModelConfig` — provider/name/base_url/api_key |
| `paths` | Config file path resolution |
| `schema` | `NimiaConfig` — top-level YAML schema |

## Key Types

- `NimiaConfig` — root config structure
- `EffectiveConfig` — config with all defaults resolved
- `BackendConfig` — per-backend enabled/command/model settings
- `RecallThresholdsConfig` — per-bucket recall confidence thresholds
- `BackendAdapter` — strategy trait for mapping backend configurations

