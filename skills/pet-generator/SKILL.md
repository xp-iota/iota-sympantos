---
name: pet-generator
description: Use when the user requests "生成宠物", "generate pet", or "create pet" and expects a generated pet assembled from iota-fun MCP tool results.
triggers:
  - 生成宠物
  - generate pet
  - create pet
execution:
  mode: mcp
  server: iota-fun
  parallel: true
  tools:
    - name: fun.cpp
      as: action
    - name: fun.typescript
      as: color
    - name: fun.rust
      as: material
    - name: fun.zig
      as: size
    - name: fun.java
      as: animal
    - name: fun.python
      as: lengthCm
    - name: fun.go
      as: toyShape
output:
  template: |
    一只正在{{action}}的、{{color}}的、{{material}}感的、{{size}}号的{{animal}}，抱着一个 {{lengthCm}} 厘米、{{toyShape}} 的飞盘。

    属性：
    - action: {{action}}
    - color: {{color}}
    - material: {{material}}
    - size: {{size}}
    - animal: {{animal}}
    - lengthCm: {{lengthCm}}
    - toyShape: {{toyShape}}
failurePolicy: report
---

# Pet Generator

## Purpose

当请求中包含"生成宠物"、"generate pet" 或 "create pet" 时，必须使用 `fun.*` MCP 工具获取真实随机属性值，并组合成一只完整的宠物描述。IotaEngine 会按 frontmatter 的 `execution` 声明编排 MCP 调用；普通 backend MCP 路径中也只能直接调用当前会话中的 `fun.*` 工具。

**重要：不要凭空编造属性值，不要使用文档中的示例值。** 每个属性必须来自对应工具的真实调用结果。

## Available Tools

以下工具由 `iota-fun` MCP server 暴露。调用工具时只使用 `fun.*` 工具名；不要自己读取源码、编译或执行本地文件。

| 工具名           | 返回属性         | 示例输出                                                |
| ---------------- | ---------------- | ------------------------------------------------------- |
| `fun.cpp`        | action（动作）   | `睡觉` / `奔跑` / `喝水` / `吃饭` / `捕捉` / `发呆`     |
| `fun.typescript` | color（颜色）    | `red` / `blue` / `green` / `yellow` / `black` / `white` |
| `fun.rust`       | material（材质） | `wood` / `metal` / `glass` / `plastic` / `stone`        |
| `fun.zig`        | size（尺寸）     | `大` / `中` / `小`                                      |
| `fun.java`       | animal（动物）   | `猫` / `狗` / `鸟`                                      |
| `fun.python`     | lengthCm（数字） | `1`–`100` 的随机整数                                    |
| `fun.go`         | toyShape（形状） | `circle` / `square` / `triangle` / `star` / `hexagon`   |

## Execution

获取全部 7 个属性的真实值，然后组合宠物描述。通用 `SkillRunner` 会按 frontmatter 的 `execution.tools` 调用 `fun.*` MCP 工具；普通 backend MCP 路径中，backend LLM 也只能直接调用当前会话中的 `fun.*` MCP 工具。

| 工具 / 程序      | 获取属性 | 源文件路径                                      |
| ---------------- | -------- | ----------------------------------------------- |
| `fun.cpp`        | action   | `iota-skill/pet-generator/iota-fun/cpp/`        |
| `fun.typescript` | color    | `iota-skill/pet-generator/iota-fun/typescript/` |
| `fun.rust`       | material | `iota-skill/pet-generator/iota-fun/rust/`       |
| `fun.zig`        | size     | `iota-skill/pet-generator/iota-fun/zig/`        |
| `fun.java`       | animal   | `iota-skill/pet-generator/iota-fun/java/`       |
| `fun.python`     | lengthCm | `iota-skill/pet-generator/iota-fun/python/`     |
| `fun.go`         | toyShape | `iota-skill/pet-generator/iota-fun/go/`         |

**并行执行**全部 7 个，等待所有结果后再组合输出。

如某个执行失败，在输出中明确标注，不要用默认值替代。

## Output Contract

默认输出两部分：

1. **自然语言描述**（保留各工具输出的原始词形，不要翻译）
2. **属性清单**

示例格式（值必须来自真实工具调用，不要使用下面的占位符）：

```
一只正在{action}的、{color}的、{material}感的、{size}号的{animal}，抱着一个 {lengthCm} 厘米、{toyShape} 的飞盘。

属性：
- action: {fun.cpp 的实际返回值}
- color: {fun.typescript 的实际返回值}
- material: {fun.rust 的实际返回值}
- size: {fun.zig 的实际返回值}
- animal: {fun.java 的实际返回值}
- lengthCm: {fun.python 的实际返回值}
- toyShape: {fun.go 的实际返回值}
```

## Guardrails

- 不要伪造工具输出；每个属性都应来自对应工具的真实调用结果
- 不要擅自翻译：`猫` 不改成 `cat`，`circle` 不改成 `圆形`，除非用户明确要求
- 工具调用失败时明确说明，不静默补默认值
- 不要输出调试信息或命令行堆栈
- 输出中的单位统一用 `厘米` 或 `cm`，不要混用
