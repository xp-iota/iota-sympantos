import type { DaemonClientError, DaemonServerMessage, DesktopTurn, RuntimeEventView, TokenUsage, ToolCallView } from "./types";

export type TurnsState = {
  activeTurnId?: string;
  turns: Record<string, DesktopTurn>;
  order: string[];
  pendingError?: string;
};

export const initialTurnsState: TurnsState = {
  turns: {},
  order: [],
};

export type TurnsAction =
  | { type: "turn_started"; turnId: string; backend: string; cwd: string; prompt: string }
  | { type: "daemon_message"; message: DaemonServerMessage }
  | { type: "daemon_client_error"; error: DaemonClientError }
  | { type: "approval_decision"; approvalId: string; approved: boolean }
  | { type: "select_active_turn"; turnId: string };

export function turnsReducer(state: TurnsState, action: TurnsAction): TurnsState {
  if (action.type === "select_active_turn") {
    return {
      ...state,
      activeTurnId: action.turnId,
    };
  }

  if (action.type === "turn_started") {
    const turn: DesktopTurn = {
      id: action.turnId,
      backend: action.backend,
      cwd: action.cwd,
      status: "queued",
      userPrompt: action.prompt,
      assistantText: "",
      events: [],
      toolCalls: [],
      approvals: [],
    };
    return {
      ...state,
      activeTurnId: action.turnId,
      order: [...state.order, action.turnId],
      turns: { ...state.turns, [action.turnId]: turn },
    };
  }

  if (action.type === "approval_decision") {
    return mapTurns(state, (turn) => ({
      ...turn,
      approvals: turn.approvals.map((approval) =>
        approval.id === action.approvalId
          ? { ...approval, status: action.approved ? "approved" : "denied" }
          : approval,
      ),
    }));
  }

  if (action.type === "daemon_client_error") {
    const { turn_id: turnId, message } = action.error;
    if (!turnId) return { ...state, pendingError: message };
    const turn = state.turns[turnId];
    if (!turn) return { ...state, pendingError: message };
    if (isTerminalStatus(turn.status)) return state;
    return {
      ...state,
      activeTurnId: turnId,
      pendingError: message,
      turns: {
        ...state.turns,
        [turnId]: { ...turn, status: "failed", error: message },
      },
    };
  }

  const message = action.message;
  if (message.type === "protocol_error") {
    return { ...state, pendingError: message.message };
  }
  if (message.type === "turn_started" && !state.turns[message.turn_id]) {
    const turn: DesktopTurn = {
      id: message.turn_id,
      backend: "unknown",
      cwd: "",
      status: "running",
      userPrompt: "",
      assistantText: "",
      events: [],
      toolCalls: [],
      approvals: [],
    };
    return {
      ...state,
      activeTurnId: message.turn_id,
      order: [...state.order, message.turn_id],
      turns: { ...state.turns, [message.turn_id]: turn },
    };
  }
  if (!("turn_id" in message)) {
    return state;
  }

  const existing = state.turns[message.turn_id];
  if (!existing) return state;

  const updated = reduceTurn(existing, message);
  const pendingError =
    message.type === "turn_cancelled" && !message.accepted
      ? `turn ${message.turn_id} is not active`
      : state.pendingError;
  return {
    ...state,
    activeTurnId: message.turn_id,
    pendingError,
    turns: { ...state.turns, [message.turn_id]: updated },
  };
}

function reduceTurn(turn: DesktopTurn, message: Extract<DaemonServerMessage, { turn_id: string }>): DesktopTurn {
  switch (message.type) {
    case "turn_started":
      return { ...turn, status: "running" };
    case "text_chunk":
      return { ...turn, status: "running", assistantText: turn.assistantText + message.chunk };
    case "turn_event":
      return applyRuntimeEvent({ ...turn, events: [...turn.events, message.event] }, message.event);
    case "approval_requested":
      return {
        ...turn,
        status: "waiting_approval",
        approvals: [
          ...turn.approvals,
          { id: message.approval_id, toolName: message.tool_name, params: message.params, status: "pending" },
        ],
      };
    case "turn_completed":
      return { ...turn, status: "completed", assistantText: message.text, timing: message.timing };
    case "turn_failed":
      return { ...turn, status: "failed", error: message.error };
    case "turn_cancelled":
      return message.accepted ? { ...turn, status: "cancelled" } : turn;
  }
}

function applyRuntimeEvent(turn: DesktopTurn, event: RuntimeEventView): DesktopTurn {
  if (event.kind === "TokenUsage" && isTokenUsage(event.data)) {
    return { ...turn, usage: event.data };
  }
  if (event.kind === "ToolCall" && isObject(event.data)) {
    const toolCall: ToolCallView = {
      id: String(event.data.id ?? ""),
      name: String(event.data.name ?? ""),
      arguments: event.data.arguments,
    };
    return { ...turn, toolCalls: [...turn.toolCalls, toolCall] };
  }
  if (event.kind === "ToolResult" && isObject(event.data)) {
    // Hoist the narrowed payload: the narrowing does not survive into the
    // `map` callback below, which would widen it back to `unknown`.
    const data = event.data;
    return {
      ...turn,
      toolCalls: turn.toolCalls.map((tool) =>
        tool.id === data.id
          ? { ...tool, ok: Boolean(data.ok), result: data.result }
          : tool,
      ),
    };
  }
  if (event.kind === "ApprovalDecision" && isObject(event.data)) {
    const requestId = String(event.data.request_id ?? "");
    const approved = Boolean(event.data.approved);
    return {
      ...turn,
      approvals: turn.approvals.map((approval) =>
        approval.id === requestId
          ? { ...approval, status: approved ? "approved" : "denied" }
          : approval,
      ),
    };
  }
  return turn;
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

/// Runtime events cross the wire as free-form JSON, so `TokenUsage` payloads
/// arrive typed as `unknown`. Narrow to the fields the inspector renders
/// instead of trusting the payload shape.
function isTokenUsage(value: unknown): value is TokenUsage {
  if (!isObject(value)) return false;
  return TOKEN_USAGE_NUMERIC_FIELDS.every(
    (field) => value[field] === undefined || typeof value[field] === "number",
  );
}

const TOKEN_USAGE_NUMERIC_FIELDS = [
  "input_tokens",
  "output_tokens",
  "total_tokens",
  "thinking_tokens",
  "cache_read_input_tokens",
  "cache_creation_input_tokens",
  "normalized_total_tokens",
] as const;

function mapTurns(state: TurnsState, f: (turn: DesktopTurn) => DesktopTurn): TurnsState {
  const turns: Record<string, DesktopTurn> = {};
  for (const id of Object.keys(state.turns)) {
    turns[id] = f(state.turns[id]);
  }
  return { ...state, turns };
}

function isTerminalStatus(status: DesktopTurn["status"]): boolean {
  return status === "completed" || status === "failed" || status === "cancelled";
}
