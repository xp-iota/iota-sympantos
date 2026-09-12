import assert from "node:assert/strict";
import test from "node:test";
import { initialTurnsState, turnsReducer } from "./turnReducer";

test("text_chunk appends assistant text for the matching turn", () => {
  const started = turnsReducer(initialTurnsState, {
    type: "turn_started",
    turnId: "turn-1",
    backend: "gemini",
    cwd: "/tmp/project",
    prompt: "hello",
  });

  const updated = turnsReducer(started, {
    type: "daemon_message",
    message: { type: "text_chunk", turn_id: "turn-1", chunk: "hi" },
  });

  assert.equal(updated.turns["turn-1"].assistantText, "hi");
  assert.equal(updated.turns["turn-1"].status, "running");
});

test("approval_requested marks the turn as waiting for approval", () => {
  const started = turnsReducer(initialTurnsState, {
    type: "turn_started",
    turnId: "turn-1",
    backend: "gemini",
    cwd: "/tmp/project",
    prompt: "hello",
  });

  const updated = turnsReducer(started, {
    type: "daemon_message",
    message: {
      type: "approval_requested",
      turn_id: "turn-1",
      approval_id: "approval-1",
      tool_name: "shell",
      params: { command: "ls" },
    },
  });

  assert.equal(updated.turns["turn-1"].status, "waiting_approval");
  assert.equal(updated.turns["turn-1"].approvals[0].id, "approval-1");
});

test("turn_completed stores timing and completes the turn", () => {
  const started = turnsReducer(initialTurnsState, {
    type: "turn_started",
    turnId: "turn-1",
    backend: "codex",
    cwd: "/tmp/project",
    prompt: "hello",
  });

  const updated = turnsReducer(started, {
    type: "daemon_message",
    message: {
      type: "turn_completed",
      turn_id: "turn-1",
      text: "final",
      timing: { total_ms: 12, client_started: true, process_spawned: true, session_reused: false, prompt_ms: 10 },
    },
  });

  assert.equal(updated.turns["turn-1"].status, "completed");
  assert.equal(updated.turns["turn-1"].assistantText, "final");
});

test("turn_failed preserves partial text and stores error", () => {
  const started = turnsReducer(initialTurnsState, {
    type: "turn_started",
    turnId: "turn-1",
    backend: "codex",
    cwd: "/tmp/project",
    prompt: "hello",
  });
  const chunked = turnsReducer(started, {
    type: "daemon_message",
    message: { type: "text_chunk", turn_id: "turn-1", chunk: "partial" },
  });

  const failed = turnsReducer(chunked, {
    type: "daemon_message",
    message: { type: "turn_failed", turn_id: "turn-1", error: "boom" },
  });

  assert.equal(failed.turns["turn-1"].status, "failed");
  assert.equal(failed.turns["turn-1"].assistantText, "partial");
  assert.equal(failed.turns["turn-1"].error, "boom");
});

test("select_active_turn updates activeTurnId", () => {
  const started = turnsReducer(initialTurnsState, {
    type: "turn_started",
    turnId: "turn-1",
    backend: "gemini",
    cwd: "/tmp/project",
    prompt: "hello",
  });

  const selected = turnsReducer(started, {
    type: "select_active_turn",
    turnId: "turn-1",
  });

  assert.equal(selected.activeTurnId, "turn-1");
});

test("turn_started daemon message creates a placeholder when local turn is not registered yet", () => {
  const updated = turnsReducer(initialTurnsState, {
    type: "daemon_message",
    message: { type: "turn_started", turn_id: "turn-1" },
  });

  assert.equal(updated.turns["turn-1"].status, "running");
  assert.equal(updated.activeTurnId, "turn-1");
});

test("turn_cancelled with accepted false keeps running status and records protocol error", () => {
  const started = turnsReducer(initialTurnsState, {
    type: "turn_started",
    turnId: "turn-1",
    backend: "gemini",
    cwd: "/tmp/project",
    prompt: "hello",
  });

  const updated = turnsReducer(started, {
    type: "daemon_message",
    message: { type: "turn_cancelled", turn_id: "turn-1", accepted: false },
  });

  assert.equal(updated.turns["turn-1"].status, "queued");
  assert.match(updated.pendingError ?? "", /not active/);
});

test("turn_cancelled with accepted true marks the turn cancelled", () => {
  const started = turnsReducer(initialTurnsState, {
    type: "turn_started",
    turnId: "turn-1",
    backend: "gemini",
    cwd: "/tmp/project",
    prompt: "hello",
  });
  const running = turnsReducer(started, {
    type: "daemon_message",
    message: { type: "turn_started", turn_id: "turn-1" },
  });

  const updated = turnsReducer(running, {
    type: "daemon_message",
    message: { type: "turn_cancelled", turn_id: "turn-1", accepted: true },
  });

  assert.equal(updated.turns["turn-1"].status, "cancelled");
});

test("daemon_client_error without turn records pending error", () => {
  const updated = turnsReducer(initialTurnsState, {
    type: "daemon_client_error",
    error: { message: "daemon unavailable" },
  });

  assert.equal(updated.pendingError, "daemon unavailable");
});

test("daemon_client_error marks a running turn failed and preserves partial text", () => {
  const started = turnsReducer(initialTurnsState, {
    type: "turn_started",
    turnId: "turn-1",
    backend: "gemini",
    cwd: "/tmp/project",
    prompt: "hello",
  });
  const chunked = turnsReducer(started, {
    type: "daemon_message",
    message: { type: "text_chunk", turn_id: "turn-1", chunk: "partial" },
  });

  const failed = turnsReducer(chunked, {
    type: "daemon_client_error",
    error: { turn_id: "turn-1", message: "stream disconnected" },
  });

  assert.equal(failed.turns["turn-1"].status, "failed");
  assert.equal(failed.turns["turn-1"].assistantText, "partial");
  assert.equal(failed.turns["turn-1"].error, "stream disconnected");
  assert.equal(failed.pendingError, "stream disconnected");
});

test("daemon_client_error does not change a completed turn", () => {
  const started = turnsReducer(initialTurnsState, {
    type: "turn_started",
    turnId: "turn-1",
    backend: "gemini",
    cwd: "/tmp/project",
    prompt: "hello",
  });
  const completed = turnsReducer(started, {
    type: "daemon_message",
    message: { type: "turn_completed", turn_id: "turn-1", text: "done", timing: { total_ms: 0, client_started: false, process_spawned: false, session_reused: false, prompt_ms: 0 } },
  });

  const unchanged = turnsReducer(completed, {
    type: "daemon_client_error",
    error: { turn_id: "turn-1", message: "late eof" },
  });

  assert.equal(unchanged.turns["turn-1"].status, "completed");
  assert.equal(unchanged.turns["turn-1"].assistantText, "done");
  assert.equal(unchanged.pendingError, undefined);
});

test("turn_event ToolCall adds inspector tool call", () => {
  const started = turnsReducer(initialTurnsState, {
    type: "turn_started",
    turnId: "turn-1",
    backend: "gemini",
    cwd: "/tmp/project",
    prompt: "hello",
  });

  const updated = turnsReducer(started, {
    type: "daemon_message",
    message: {
      type: "turn_event",
      turn_id: "turn-1",
      event: { kind: "ToolCall", data: { id: "tool-1", name: "shell", arguments: { command: "ls" } } },
    },
  });

  assert.equal(updated.turns["turn-1"].toolCalls.length, 1);
  assert.equal(updated.turns["turn-1"].toolCalls[0].name, "shell");
});

test("turn_event ToolResult updates matching tool call", () => {
  const started = turnsReducer(initialTurnsState, {
    type: "turn_started",
    turnId: "turn-1",
    backend: "gemini",
    cwd: "/tmp/project",
    prompt: "hello",
  });
  const called = turnsReducer(started, {
    type: "daemon_message",
    message: {
      type: "turn_event",
      turn_id: "turn-1",
      event: { kind: "ToolCall", data: { id: "tool-1", name: "shell", arguments: {} } },
    },
  });

  const updated = turnsReducer(called, {
    type: "daemon_message",
    message: {
      type: "turn_event",
      turn_id: "turn-1",
      event: { kind: "ToolResult", data: { id: "tool-1", ok: true, result: { output: "ok" } } },
    },
  });

  assert.equal(updated.turns["turn-1"].toolCalls[0].ok, true);
  assert.deepEqual(updated.turns["turn-1"].toolCalls[0].result, { output: "ok" });
});

test("turn_event TokenUsage stores usage", () => {
  const started = turnsReducer(initialTurnsState, {
    type: "turn_started",
    turnId: "turn-1",
    backend: "gemini",
    cwd: "/tmp/project",
    prompt: "hello",
  });

  const updated = turnsReducer(started, {
    type: "daemon_message",
    message: {
      type: "turn_event",
      turn_id: "turn-1",
      event: { kind: "TokenUsage", data: { input_tokens: 10, output_tokens: 5, total_tokens: 15 } },
    },
  });

  assert.equal(updated.turns["turn-1"].usage?.total_tokens, 15);
});

test("turn_event ApprovalDecision updates matching approval status", () => {
  const started = turnsReducer(initialTurnsState, {
    type: "turn_started",
    turnId: "turn-1",
    backend: "gemini",
    cwd: "/tmp/project",
    prompt: "hello",
  });
  const requested = turnsReducer(started, {
    type: "daemon_message",
    message: {
      type: "approval_requested",
      turn_id: "turn-1",
      approval_id: "approval-1",
      tool_name: "shell",
      params: {},
    },
  });

  const updated = turnsReducer(requested, {
    type: "daemon_message",
    message: {
      type: "turn_event",
      turn_id: "turn-1",
      event: { kind: "ApprovalDecision", data: { request_id: "approval-1", approved: false } },
    },
  });

  assert.equal(updated.turns["turn-1"].approvals[0].status, "denied");
});
