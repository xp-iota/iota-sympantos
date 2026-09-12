export type TurnStatus = "queued" | "running" | "waiting_approval" | "completed" | "failed" | "cancelled";

/// Timing breakdown for a completed ACP prompt, mirroring
/// `iota_core::acp::AcpPromptTiming` (Rust) exactly so this stays a
/// concrete, checked type rather than `any` (result.md S-07 / AC11.1).
export type AcpPromptTiming = {
  client_started: boolean;
  process_spawned: boolean;
  process_spawn_ms?: number;
  init_ms?: number;
  session_reused: boolean;
  session_new_ms?: number;
  prompt_ms: number;
  total_ms: number;
};

/// Token usage payload carried by a `TokenUsage` runtime event, mirroring
/// `iota_core::runtime_event::TokenUsageEvent` (Rust).
export type TokenUsage = {
  provider?: string;
  backend?: string;
  execution_id?: string;
  session_id?: string;
  source?: string;
  input_tokens?: number;
  output_tokens?: number;
  normalized_total_tokens?: number;
  total_tokens?: number;
  thinking_tokens?: number;
  cache_read_input_tokens?: number;
  cache_creation_input_tokens?: number;
};

/// Runtime event payloads are inherently free-form JSON on the wire (ACP
/// backends and MCP tool arguments/results are not schema-constrained at
/// this layer in Rust either — see `serde_json::Value` fields on
/// `RuntimeEvent` variants). Rather than widen this back to `any`, model it
/// as `unknown` so every access site is forced through an explicit runtime
/// check (see `isObject`/narrowing helpers in `turnReducer.ts`) instead of
/// silently type-checking as anything. This is the deliberate boundary
/// documented in result.md S-07 / AC11.2 for the one place a static shape
/// genuinely cannot be assigned.
export type RuntimeEventView = {
  kind: string;
  data: unknown;
};

export type ApprovalView = {
  id: string;
  toolName: string;
  /// Free-form ACP `session/request_permission` params — see
  /// `RuntimeEventView.data` doc above for why this is `unknown` rather
  /// than a concrete type or `any`.
  params: unknown;
  status: "pending" | "approved" | "denied";
};

export type ToolCallView = {
  id: string;
  name: string;
  /// Tool call arguments/results are arbitrary JSON defined by whichever
  /// MCP tool was invoked; see `RuntimeEventView.data` doc above.
  arguments: unknown;
  ok?: boolean;
  result?: unknown;
};

export type DesktopTurn = {
  id: string;
  backend: string;
  cwd: string;
  status: TurnStatus;
  userPrompt: string;
  assistantText: string;
  events: RuntimeEventView[];
  toolCalls: ToolCallView[];
  approvals: ApprovalView[];
  timing?: AcpPromptTiming;
  usage?: TokenUsage;
  error?: string;
};

export type DaemonConnectionState = "connected" | "reconnecting" | "disconnected";

/// Machine-readable failure class from the daemon. Mirrors
/// `iota_core::daemon::DaemonErrorCode`.
export type DaemonErrorCode =
  | "unsupported_version"
  | "invalid_request"
  | "unauthenticated"
  | "not_found"
  | "unavailable"
  | "cancelled"
  | "backend_error"
  | "limit_exceeded"
  | "internal";

export type DaemonServerMessage =
  | { type: "hello_accepted"; protocol_version: number; negotiated_version?: number }
  | {
      type: "protocol_error";
      message: string;
      /// Absent only for messages from a daemon predating the field.
      code?: DaemonErrorCode;
      /// Whether retrying the same request could succeed. Terminal failures
      /// (bad version, malformed, unauthenticated) report false so the client
      /// does not loop.
      retryable?: boolean;
      /// Correlation id of the request this failure answers, when the request
      /// supplied one.
      request_id?: string;
    }
  | { type: "turn_started"; turn_id: string }
  | { type: "text_chunk"; turn_id: string; chunk: string }
  | { type: "turn_event"; turn_id: string; event: RuntimeEventView }
  | { type: "approval_requested"; turn_id: string; approval_id: string; tool_name: string; params: unknown }
  | { type: "approval_responded"; approval_id: string; accepted: boolean }
  | { type: "turn_completed"; turn_id: string; text: string; timing: AcpPromptTiming }
  | { type: "turn_failed"; turn_id: string; error: string }
  | { type: "turn_cancelled"; turn_id: string; accepted: boolean }
  | { type: "config_snapshot"; config: DesktopConfigSnapshot }
  | { type: "backend_check_result"; backend: string; ok: boolean; details: string }
  | { type: "observability_summary"; summary: ObservabilitySummary }
  | { type: "memory_context_snapshot"; snapshot: DesktopMemoryContextSnapshot }
  | { type: "pong"; seq: number };

export type DaemonClientError = {
  turn_id?: string;
  message: string;
};

export type DesktopModelConfig = {
  provider?: string;
  name?: string;
  base_url?: string;
  api_key_configured: boolean;
  api_key_update?: string;
};

export type DesktopBackendSnapshot = {
  backend: string;
  enabled: boolean;
  model?: DesktopModelConfig;
};

export type DesktopConfigSnapshot = {
  config_path: string;
  backends: Record<string, DesktopBackendSnapshot>;
};

export type BackendCheckResult = {
  backend: string;
  ok: boolean;
  details: string;
};

export type KanbanStatus = "triage" | "todo" | "ready" | "running" | "blocked" | "done" | "archived";

export type KanbanBoard = {
  id: number;
  slug: string;
  name: string;
  created_at: number;
};

export type KanbanTask = {
  id: number;
  board_id: number;
  title: string;
  body?: string;
  status: KanbanStatus;
  assignee?: string;
  priority: number;
  tags: string[];
  workspace_kind?: string;
  workspace_path?: string;
  created_at: number;
  updated_at: number;
  claimed_at?: number;
  claim_ttl_secs: number;
};

export type KanbanTaskPatch = {
  title?: string;
  body?: string | null;
  status?: KanbanStatus;
  assignee?: string | null;
  priority?: number;
  tags?: string[];
  workspace_kind?: string | null;
  workspace_path?: string | null;
};

export type KanbanComment = {
  id: number;
  task_id: number;
  author: string;
  body: string;
  created_at: number;
};

export type KanbanRunStatus = "running" | "completed" | "failed" | "timed_out" | "cancelled";

export type KanbanRun = {
  id: string;
  task_id: number;
  profile: string;
  status: KanbanRunStatus;
  started_at: number;
  finished_at?: number;
  last_heartbeat: number;
  exit_code?: number;
  output_summary?: string;
};

export type KanbanLinkKind = "parent" | "blocks" | "related";

export type KanbanLink = {
  from_id: number;
  to_id: number;
  kind: KanbanLinkKind;
};

export type KanbanCreateLinkRequest = {
  from_id: number;
  to_id: number;
  kind: KanbanLinkKind;
};

export type KanbanEvent = {
  id: number;
  event_type: string;
  payload: string;
  created_at: number;
};

export type KanbanTaskLogs = {
  stdout_path: string;
  stderr_path: string;
  stdout: string;
  stderr: string;
};

export type KanbanTaskDetail = {
  task: KanbanTask;
  board?: KanbanBoard;
  comments: KanbanComment[];
  runs: KanbanRun[];
  links: KanbanLink[];
  events: KanbanEvent[];
  logs: KanbanTaskLogs;
};

export type KanbanTaskFilter = {
  board_id?: number;
  status?: KanbanStatus;
  assignee?: string;
  limit?: number;
};

export type KanbanDispatchReport = {
  spawned: number;
  completed: number;
  timed_out: number;
  spawn_failures: number;
  reclaimed: number;
  active_workers: number;
};

export type ObservabilitySummary = {
  cwd?: string;
  window_secs?: number;
  token_summary?: Array<{
    backend: string;
    count: number;
    normalized_total_mean?: number;
    input_tokens_mean?: number;
    output_tokens_mean?: number;
  }>;
  recent_token_executions?: Array<{
    id: string;
    ts: number;
    execution_id?: string;
    backend: string;
    model?: string;
    normalized_total_tokens?: number;
  }>;
  error?: string;
};

export type DesktopMemoryRecord = {
  id: string;
  type: string;
  facet?: string;
  scope: string;
  scope_id: string;
  content: string;
  confidence: number;
  created_at: number;
  updated_at: number;
  expires_at: number;
};

export type DesktopMemoryBuckets = {
  identity: DesktopMemoryRecord[];
  preference: DesktopMemoryRecord[];
  strategic: DesktopMemoryRecord[];
  domain: DesktopMemoryRecord[];
  procedural: DesktopMemoryRecord[];
  episodic: DesktopMemoryRecord[];
};

export type DesktopMemorySummary = {
  identity: number;
  preference: number;
  strategic: number;
  domain: number;
  procedural: number;
  episodic: number;
};

export type DesktopContextBudgetsSnapshot = {
  memory_chars: number;
  skills_chars: number;
  working_memory_chars: number;
  workspace_chars: number;
  handoff_chars: number;
};

export type DesktopContextSection = {
  name: string;
  chars: number;
  preview: string;
};

export type DesktopRuntimeContextSnapshot = {
  turn_id: string;
  backend: string;
  cwd: string;
  session_id: string;
  model?: string;
  created_at: number;
  capsule_text: string;
  sections: DesktopContextSection[];
  budgets: DesktopContextBudgetsSnapshot;
};

export type DesktopContextEngineSnapshot = {
  enabled: boolean;
  memory_db?: string;
  budgets: DesktopContextBudgetsSnapshot;
};

export type DesktopSnapshotError = {
  area: string;
  message: string;
};

export type DesktopMemoryContextSnapshot = {
  cwd: string;
  scope_mode: "workspace" | "all";
  memory: DesktopMemoryBuckets;
  memory_summary: DesktopMemorySummary;
  runtime_context?: DesktopRuntimeContextSnapshot;
  context_engine: DesktopContextEngineSnapshot;
  errors: DesktopSnapshotError[];
};
