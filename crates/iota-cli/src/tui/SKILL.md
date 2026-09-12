---
name: iota-src-tui
description: Use when working on ratatui interactive UI, composer key handling, markdown rendering, overlays, terminal lifecycle, frame loop, or files under crates/iota-cli/src/tui.
triggers:
  - crates/iota-cli/src/tui
  - TUI
  - ratatui
  - input
  - status_bar
  - terminal_lifecycle
  - loop
---

# tui — Interactive Terminal UI

ratatui-based interactive chat interface with markdown rendering, multi-line composer, and backend selector.

## Layout

```
┌─ header ──────────────────── 1 row ─┐
│  magenta bg · cwd · active backend  │
├─ history ──────────────── fill rows ─┤
│  scrollable conversation transcript  │
├─ input ──────────────── 3+ rows ──┤
│  multi-line input with cursor        │
├─ status ───────────────── 1 row ────┤
│  backend·model  /  key hints         │
└─────────────────────────────────────┘
```

## Features

- Multi-line input (Shift+Enter), Unicode grapheme cursor
- Kill buffer (Ctrl+K/Y), word delete (Ctrl+U/W), word motion (Alt+B/F)
- Ctrl+R incremental history search
- Markdown rendering (pulldown-cmark)
- Approval overlay for tool permission requests
- Ctrl+T full-screen pager, ? help overlay
- Tab queue (buffer input while backend is running)
- ~120 FPS frame limiter, mouse capture
- Local slash commands including `/memory` (alias `/mem`) for memory recall and hybrid search.

## Sub-modules

| Module | Purpose |
| :--------| :---------|
| `autocomplete` | Slash command autocompletions, completion hints, and ghost text |
| `exporter` | Formatting and exporting TUI chat transcript text files |
| `input` | Multi-line input with kill-buffer, word motion, history search |
| `events` | Keyboard/mouse event handling and dispatch |
| `loop` | Main event loop, async engine turn, frame timing |
| `markdown` | pulldown-cmark rendering to ratatui spans |
| `render` | Widget layout and drawing |
| `state` | TUI application state |
| `status_bar` | Bottom status bar (backend·model, key hints) |
| `terminal_lifecycle` | Terminal setup/restore RAII guard |
| `theme` | ratatui color theme (magenta accent) |

