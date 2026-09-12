# iota Fun - Multi-Language Function Examples

> **Version:** 1.0
> **Last Updated:** 2026-05-05

## Overview

`iota-fun/` contains example functions in multiple programming languages that demonstrate iota's multi-language execution capabilities. These functions are exposed by the Rust `iota mcp fun` stdio server (`iota fun-mcp` is kept as a compatibility alias) and can be called by engine-run skills through `crates/iota-core/src/skill/runner.rs` and `crates/iota-core/src/mcp/client.rs`.

## Purpose

- **Demonstrate multi-language support**: Show how iota can execute code in Python, TypeScript, Go, Rust, Zig, Java, and C++
- **Provide simple examples**: Each function generates a random value (number, color, shape, material, size, animal, action)
- **Enable testing**: Serve as test cases for the fun-call routing system
- **Educational reference**: Help developers understand how to add new language support

## Directory Structure

```
iota-fun/
├── python/
│   └── random_number.py          # Generates random number (1-100)
├── typescript/
│   ├── randomColor.ts            # Generates random color
│   └── runner.js                 # TypeScript runner
├── go/
│   ├── random_shape.go           # Generates random shape
│   └── runner.go                 # Go runner
├── rust/
│   ├── random_material.rs        # Generates random material
│   └── runner.rs                 # Rust runner
├── zig/
│   ├── random_size.zig           # Generates random size
│   └── runner.zig                # Zig runner
├── java/
│   ├── RandomAnimal.java         # Generates random animal
│   └── RandomAnimalRunner.java   # Java runner
├── cpp/
│   ├── random_action.cpp         # Generates random action
│   └── random_action_runner.cpp  # C++ runner
└── README.md                     # This file
```

## Supported Languages and Functions

| Language   | Function             | Output Example | File                        |
| ---------- | -------------------- | -------------- | --------------------------- |
| Python     | `random_number()`    | `42`           | `python/random_number.py`   |
| TypeScript | `randomColor()`      | `"blue"`       | `typescript/randomColor.ts` |
| Go         | `RandomShape()`      | `"circle"`     | `go/random_shape.go`        |
| Rust       | `random_material()`  | `"metal"`      | `rust/random_material.rs`   |
| Zig        | `randomSize()`       | `"large"`      | `zig/random_size.zig`       |
| Java       | `RandomAnimal.get()` | `"elephant"`   | `java/RandomAnimal.java`    |
| C++        | `randomAction()`     | `"jump"`       | `cpp/random_action.cpp`     |

## Usage

### MCP Server

Start the MCP server from the Rust CLI:

```bash
iota mcp fun
```

The server speaks stdio JSON-RPC and exposes the tools used by `pet-generator`:

```text
fun.python
fun.typescript
fun.go
fun.rust
fun.zig
fun.java
fun.cpp
```

### Engine-Run Skill

When a prompt matches `skills/pet-generator/SKILL.md`, `IotaEngine` loads the skill, `crates/iota-core/src/skill/runner.rs` starts the iota-fun MCP sidecar, and `crates/iota-core/src/mcp/client.rs` calls the declared tools. With `execution.parallel: true`, the seven tool calls are batched and the results are rendered into the skill output template.

Example prompt:

```text
生成宠物
```

This path can complete without sending the prompt to an ACP backend because the skill output is generated from MCP tool results.

## Function Details

### Python: Random Number (1-100)

**File:** `python/random_number.py`

```python
import random

def random_number() -> int:
    return random.randint(1, 100)
```

**Output:** Integer between 1 and 100

---

### TypeScript: Random Color

**File:** `typescript/randomColor.ts`

```typescript
const COLORS = ["red", "blue", "green", "yellow", "black", "white"];

export function randomColor(): string {
  const index = Math.floor(Math.random() * COLORS.length);
  return COLORS[index];
}
```

**Output:** One of: `red`, `blue`, `green`, `yellow`, `black`, `white`

---

### Go: Random Shape

**File:** `go/random_shape.go`

```go
package main

import "math/rand"

var shapes = []string{"circle", "square", "triangle", "star", "hexagon"}

func RandomShape() string {
    return shapes[rand.Intn(len(shapes))]
}
```

**Output:** One of: `circle`, `square`, `triangle`, `star`, `hexagon`

---

### Rust: Random Material

**File:** `rust/random_material.rs`

```rust
pub fn random_material() -> &'static str {
    let materials = ["wood", "metal", "glass", "plastic", "stone"];
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as usize)
        .unwrap_or(0);
    materials[nanos % materials.len()]
}
```

**Output:** One of: `wood`, `metal`, `glass`, `plastic`, `stone`

---

### Zig: Random Size

**File:** `zig/random_size.zig`

**Output:** One of: `small`, `medium`, `large`, `extra-large`

---

### Java: Random Animal

**File:** `java/RandomAnimal.java`

**Output:** One of: `elephant`, `tiger`, `dolphin`, `eagle`, `panda`

---

### C++: Random Action

**File:** `cpp/random_action.cpp`

**Output:** One of: `run`, `jump`, `swim`, `fly`, `climb`

---

## Prerequisites

To execute these functions, you need the following language runtimes installed:

| Language   | Required Runtime            | Version Check       |
| ---------- | --------------------------- | ------------------- |
| Python     | `python3`                   | `python3 --version` |
| TypeScript | `bun` or `node` + `ts-node` | `bun --version`     |
| Go         | `go`                        | `go version`        |
| Rust       | `rustc`                     | `rustc --version`   |
| Zig        | `zig`                       | `zig version`       |
| Java       | `java` + `javac`            | `java --version`    |
| C++        | `g++` or `clang++`          | `g++ --version`     |

## How It Works

### Execution Flow

1. `SkillRegistry` loads `SKILL.md` and matches prompt triggers.
2. `crates/iota-core/src/skill/runner.rs` reads `execution.tools` and starts the configured MCP server.
3. `crates/iota-core/src/mcp/client.rs` sends stdio JSON-RPC `tools/call` requests such as `fun.python` or `fun.rust`.
4. `src/fun_mcp.rs` executes the supplied source with the requested language runtime or compiler.
5. Tool results are recorded as `RuntimeEvent::ToolResult` and rendered into the skill template.

### Runtime Mapping

| Tool | Runtime path |
| :---| :---|
| `fun.python` | `python3 <verified-script>` |
| `fun.typescript` | `node -e <source>` |
| `fun.go` | writes `main.go`, runs `go run` |
| `fun.rust` | writes `main.rs`, compiles with `rustc`, runs cached binary |
| `fun.zig` | writes `main.zig`, runs `zig run` |
| `fun.java` | writes `Main.java`, compiles with `javac`, runs `java -cp` |
| `fun.cpp` | writes `main.cpp`, compiles with `clang++` or `g++`, runs cached binary |

Compiled outputs are cached under `~/.i6/fun-cache/<language>/<hash>`. Do not commit local compiled outputs.

## Adding New Languages

To add support for a new language, update `src/fun_mcp.rs`: add the tool name to `TOOLS`, add a `run_tool()` branch, implement the runtime helper, and document the new tool in this README and any skill that uses it.

## Testing

```bash
cargo test fun_mcp --lib
cargo run -- mcp fun
```

## Troubleshooting

### Common Issues

**Issue: "Command not found"**

- **Cause**: Language runtime not installed
- **Solution**: Install the required runtime (see Prerequisites)

**Issue: "Permission denied"**

- **Cause**: Runner file not executable
- **Solution**: `chmod +x iota-fun/<language>/runner.*`

**Issue: "Compilation failed"**

- **Cause**: Syntax error or missing dependencies
- **Solution**: Test the file directly with the language compiler

**Issue: "Timeout"**

- **Cause**: Function takes too long to execute
- **Solution**: Increase `timeoutMs` parameter

**Issue: "Empty output"**

- **Cause**: Function doesn't print to stdout
- **Solution**: Ensure function prints result to stdout

## Performance Considerations

- **Compilation overhead**: Compiled languages (Rust, C++, Java) have compilation overhead on first run
- **Subprocess spawn**: Each execution spawns a new subprocess (~10-50ms overhead)
- **Timeout**: Default timeout is 30 seconds, adjust based on function complexity
- **Caching**: Consider caching compiled binaries for compiled languages

## Security Considerations

- **Sandboxing**: Functions run in subprocess, not in main process
- **Timeout**: All executions have timeout to prevent hanging
- **Input validation**: Validate language parameter to prevent command injection
- **Working directory**: Functions execute in isolated `iota-fun/` directory
- **No network access**: Example functions don't require network access

## Related Documentation

- [Project architecture](../../../docs/architecture.md) - Engine, skill, and iota-fun architecture
- [Code call chains](../../../docs/code-call-chains.md) - Runtime paths for engine-run skills and MCP tools
- [iota-fun server implementation](../../../crates/iota-core/src/skill/fun.rs) - Current implementation
- [Pet generator skill](../SKILL.md) - Skill spec for multi-language pet generation

## Contributing

To contribute new language examples:

1. Follow the directory structure convention
2. Keep functions simple and deterministic
3. Add comprehensive tests
4. Update all documentation
5. Ensure cross-platform compatibility

## License

Part of the iota project. See root LICENSE file.

---

## Quick Reference

### Execute All Languages

```bash
# From iota-fun/ directory
python3 python/random_number.py
bun typescript/runner.js
cd go && go build -o "$HOME/.iota/iota-fun/iota-fun-go-manual" random_shape.go runner.go && "$HOME/.iota/iota-fun/iota-fun-go-manual" && cd ..
cd rust && rustc runner.rs -o "$HOME/.iota/iota-fun/iota-fun-rust-manual" && "$HOME/.iota/iota-fun/iota-fun-rust-manual" && cd ..
cd zig && zig build-exe runner.zig -O ReleaseFast -femit-bin="$HOME/.iota/iota-fun/iota-fun-zig-manual" && "$HOME/.iota/iota-fun/iota-fun-zig-manual" && cd ..
cd java && javac -encoding UTF-8 -d "$HOME/.iota/iota-fun/java-manual-classes" *.java && java -cp "$HOME/.iota/iota-fun/java-manual-classes" RandomAnimalRunner && cd ..
cd cpp && g++ random_action_runner.cpp -o "$HOME/.iota/iota-fun/iota-fun-cpp-manual" && "$HOME/.iota/iota-fun/iota-fun-cpp-manual" && cd ..
```

### Expected Outputs

- Python: `42` (or any number 1-100)
- TypeScript: `blue` (or any color)
- Go: `circle` (or any shape)
- Rust: `metal` (or any material)
- Zig: `large` (or any size)
- Java: `elephant` (or any animal)
- C++: `jump` (or any action)
