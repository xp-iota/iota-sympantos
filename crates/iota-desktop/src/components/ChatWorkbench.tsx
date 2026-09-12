import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";
import { AlertCircle, CheckCircle2, ChevronDown, CircleDashed, Moon, Send, Sun } from "lucide-react";
import {
  checkBackend,
  currentWorkspace,
  getConfig,
  getObservabilitySummary,
  listenDaemonClientErrors,
  listenDaemonMessages,
  submitPrompt,
} from "../api";
import { initialTurnsState, turnsReducer } from "../turnReducer";
import { applyTheme, getInitialTheme, type ColorTheme } from "../theme";
import { RightInspector, type InspectorTab } from "./RightInspector";
import { ConfigPanel } from "./ConfigPanel";
import type { BackendCheckResult, DesktopConfigSnapshot, ObservabilitySummary } from "../types";

const BACKENDS = ["gemini", "claude", "hermes", "codex", "opencode"];
const MIN_INSPECTOR_WIDTH = 360;
const MIN_WORKBENCH_WIDTH = 320;

const BRIDGE_UNAVAILABLE_DETAIL = "Desktop bridge unavailable; run through Tauri to check configured backends.";

type BackendStatus = "ready" | "unavailable" | "checking" | "unverified";

function backendStatus(check?: BackendCheckResult): BackendStatus {
  if (!check) return "checking";
  if (check.details === BRIDGE_UNAVAILABLE_DETAIL) return "unverified";
  return check.ok ? "ready" : "unavailable";
}

function backendStatusLabel(status: BackendStatus) {
  if (status === "ready") return "Ready";
  if (status === "unavailable") return "Unavailable";
  if (status === "unverified") return "Unverified";
  return "Checking";
}

function backendStatusTheme(status: BackendStatus) {
  if (status === "ready") {
    return {
      dot: "bg-emerald-400 shadow-[0_0_8px_rgba(52,211,153,0.45)]",
      text: "text-emerald-300",
      icon: "text-emerald-300",
      row: "border-emerald-500/20 bg-emerald-500/[0.07] hover:bg-emerald-500/[0.11]",
    };
  }
  if (status === "unavailable") {
    return {
      dot: "bg-rose-400 shadow-[0_0_8px_rgba(251,113,133,0.45)]",
      text: "text-rose-300",
      icon: "text-rose-300",
      row: "border-rose-500/25 bg-rose-500/[0.08] hover:bg-rose-500/[0.13]",
    };
  }
  if (status === "unverified") {
    return {
      dot: "bg-sky-300 shadow-[0_0_8px_rgba(125,211,252,0.35)]",
      text: "text-sky-300",
      icon: "text-sky-300",
      row: "border-sky-500/20 bg-sky-500/[0.07] hover:bg-sky-500/[0.11]",
    };
  }
  return {
    dot: "bg-amber-300 shadow-[0_0_8px_rgba(252,211,77,0.35)]",
    text: "text-amber-300",
    icon: "text-amber-300",
    row: "border-amber-500/20 bg-amber-500/[0.07] hover:bg-amber-500/[0.11]",
  };
}

function BackendStatusIcon({ status, className }: { status: BackendStatus; className: string }) {
  if (status === "ready") return <CheckCircle2 className={className} />;
  if (status === "unavailable") return <AlertCircle className={className} />;
  return <CircleDashed className={className} />;
}

function backendCheckErrorDetails(err: unknown) {
  const message = err instanceof Error ? err.message : String(err);
  if (message.includes("reading 'invoke'") || message.includes("reading 'transformCallback'")) {
    return BRIDGE_UNAVAILABLE_DETAIL;
  }
  return message;
}

function shouldOpenKanbanForPrompt(prompt: string) {
  const text = prompt.toLowerCase();
  return text.includes("/kanban") || text.includes("kanban") || /看板|创建.*任务|任务.*创建/.test(prompt);
}

export function clampInspectorWidth(width: number, viewportWidth = window.innerWidth) {
  const maxWidth = Math.max(MIN_INSPECTOR_WIDTH, viewportWidth - MIN_WORKBENCH_WIDTH);
  return Math.min(maxWidth, Math.max(MIN_INSPECTOR_WIDTH, width));
}

export function defaultInspectorWidth(viewportWidth = window.innerWidth) {
  return clampInspectorWidth(Math.round((viewportWidth * 2) / 3), viewportWidth);
}

export function ChatWorkbench() {
  const [theme, setTheme] = useState<ColorTheme>(getInitialTheme);
  const [state, dispatch] = useReducer(turnsReducer, initialTurnsState);
  const [backend, setBackend] = useState("hermes");
  const [input, setInput] = useState("");
  const [view, setView] = useState<"chat" | "config">("chat");
  const [config, setConfig] = useState<DesktopConfigSnapshot | null>(null);
  const [backendChecks, setBackendChecks] = useState<Record<string, BackendCheckResult>>({});
  const [observability, setObservability] = useState<ObservabilitySummary | null>(null);
  const [inspectorTab, setInspectorTab] = useState<InspectorTab>("observability");
  const [workspace, setWorkspace] = useState("");
  const [daemonStatus, setDaemonStatus] = useState<"connecting" | "connected" | "error">("connecting");
  const [inspectorWidth, setInspectorWidth] = useState(() => defaultInspectorWidth());
  const [isResizingInspector, setIsResizingInspector] = useState(false);
  const [isBackendMenuOpen, setIsBackendMenuOpen] = useState(false);
  const backendMenuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    applyTheme(theme);
  }, [theme]);

  const activeTurn = state.activeTurnId ? state.turns[state.activeTurnId] : undefined;

  const refreshBackendChecks = useCallback(async () => {
    const checks = await Promise.all(
      BACKENDS.map(async (item) => {
        try {
          return [item, await checkBackend(item)] as const;
        } catch (err) {
          return [item, { backend: item, ok: false, details: backendCheckErrorDetails(err) }] as const;
        }
      }),
    );
    setBackendChecks(Object.fromEntries(checks));
  }, []);

  const refreshObservability = useCallback(async () => {
    try {
      setObservability(await getObservabilitySummary());
    } catch (err) {
      console.error("Failed to load observability summary:", err);
    }
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let unlistenErrors: (() => void) | undefined;

    getConfig()
      .then((cfg) => {
        if (!disposed) {
          setConfig(cfg);
          setDaemonStatus("connected");
        }
      })
      .catch((err) => {
        if (!disposed) setDaemonStatus("error");
        console.error("Failed to load daemon config:", err);
      });

    currentWorkspace()
      .then((cwd) => {
        if (!disposed) setWorkspace(cwd);
      })
      .catch((err) => console.error("Failed to read workspace:", err));

    refreshObservability();
    refreshBackendChecks();

    // Listen for stream events
    listenDaemonMessages((message) => {
      if (disposed) return;
      dispatch({ type: "daemon_message", message });

      // If the message contains a config update, refresh local state
      if (message.type === "config_snapshot") {
        setConfig(message.config);
        refreshBackendChecks();
      }
      if (
        message.type === "turn_completed" ||
        message.type === "turn_failed" ||
        message.type === "turn_cancelled"
      ) {
        refreshObservability();
      }
    })
      .then((cleanup) => {
        if (disposed) {
          cleanup();
        } else {
          unlisten = cleanup;
        }
      })
      .catch((err) => console.error("Failed to listen for daemon messages:", err));

    listenDaemonClientErrors((error) => {
      if (disposed) return;
      dispatch({ type: "daemon_client_error", error });
      setDaemonStatus("error");
    })
      .then((cleanup) => {
        if (disposed) {
          cleanup();
        } else {
          unlistenErrors = cleanup;
        }
      })
      .catch((err) => console.error("Failed to listen for daemon client errors:", err));

    return () => {
      disposed = true;
      unlisten?.();
      unlistenErrors?.();
    };
  }, [refreshBackendChecks, refreshObservability]);

  useEffect(() => {
    if (!isBackendMenuOpen) return;

    const closeOnOutsidePointer = (event: PointerEvent) => {
      if (!backendMenuRef.current?.contains(event.target as Node)) {
        setIsBackendMenuOpen(false);
      }
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setIsBackendMenuOpen(false);
    };

    window.addEventListener("pointerdown", closeOnOutsidePointer);
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("pointerdown", closeOnOutsidePointer);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [isBackendMenuOpen]);

  const transcript = useMemo(() => state.order.map((id) => state.turns[id]), [state.order, state.turns]);
  const activeTurnBusy =
    activeTurn?.status === "queued" ||
    activeTurn?.status === "running" ||
    activeTurn?.status === "waiting_approval";

  async function onSubmit(event: React.FormEvent) {
    event.preventDefault();
    const prompt = input.trim();
    if (!prompt || activeTurnBusy || !canSubmit) return;
    setInput("");
    if (backend === "hermes" && shouldOpenKanbanForPrompt(prompt)) {
      setInspectorTab("kanban");
    }
    const turnId = crypto.randomUUID();
    dispatch({
      type: "turn_started",
      turnId,
      backend,
      cwd: workspace,
      prompt,
    });
    try {
      await submitPrompt(prompt, backend, turnId);
    } catch (err) {
      dispatch({
        type: "daemon_message",
        message: {
          type: "turn_failed",
          turn_id: turnId,
          error: err instanceof Error ? err.message : String(err),
        },
      });
    }
  }

  // Extract model details
  const activeBackendSnapshot = config?.backends[backend];
  const modelName = activeBackendSnapshot?.model?.name || "Loading model...";
  const isKeyConfigured = activeBackendSnapshot?.model?.api_key_configured;
  const activeBackendCheck = backendChecks[backend];
  const selectedBackendReady = activeBackendCheck?.ok === true;
  const activeBackendStatus = backendStatus(activeBackendCheck);
  const activeBackendTheme = backendStatusTheme(activeBackendStatus);
  const daemonError = state.pendingError;
  const canSubmit = !daemonError && selectedBackendReady;
  const daemonStatusText =
    daemonStatus === "connected"
      ? "Daemon Connected"
      : daemonStatus === "connecting"
        ? "Connecting"
        : "Daemon Error";

  const statusTheme =
    daemonStatus === "connected"
      ? {
          badge: "border-emerald-500/20 bg-emerald-500/5 text-emerald-400",
          dot: "bg-emerald-400",
          ping: "bg-emerald-400",
        }
      : daemonStatus === "connecting"
        ? {
            badge: "border-amber-500/20 bg-amber-500/5 text-amber-400",
            dot: "bg-amber-400",
            ping: "bg-amber-400",
          }
        : {
            badge: "border-rose-500/20 bg-rose-500/5 text-rose-400",
            dot: "bg-rose-400",
            ping: "bg-rose-400",
          };

  const handleConfigUpdate = (updated: DesktopConfigSnapshot) => {
    setConfig(updated);
    refreshBackendChecks();
  };

  const startInspectorResize = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = inspectorWidth;
    setIsResizingInspector(true);

    const onPointerMove = (moveEvent: PointerEvent) => {
      const nextWidth = startWidth - (moveEvent.clientX - startX);
      setInspectorWidth(clampInspectorWidth(nextWidth));
    };
    const onPointerUp = () => {
      setIsResizingInspector(false);
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
    };

    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);
  }, [inspectorWidth]);

  const workspaceControls = (
    <div className="flex flex-wrap items-center gap-2">
      <nav className="flex items-center bg-slate-950/40 border border-slate-800/80 p-0.5 rounded-lg">
        <button
          className={`rounded-md px-3.5 py-1 text-xs font-medium transition-all cursor-pointer ${
            view === "chat" ? "bg-primary text-white shadow-sm shadow-primary/20" : "text-slate-400 hover:text-slate-200"
          }`}
          onClick={() => setView("chat")}
        >
          Chat
        </button>
        <button
          className={`rounded-md px-3.5 py-1 text-xs font-medium transition-all cursor-pointer ${
            view === "config" ? "bg-primary text-white shadow-sm shadow-primary/20" : "text-slate-400 hover:text-slate-200"
          }`}
          onClick={() => setView("config")}
        >
          Config
        </button>
      </nav>

      <div ref={backendMenuRef} className="relative">
        <button
          type="button"
          aria-haspopup="listbox"
          aria-expanded={isBackendMenuOpen}
          onClick={() => setIsBackendMenuOpen((open) => !open)}
          className="flex h-8.5 min-w-[150px] items-center justify-between gap-2 rounded-lg border border-slate-800/80 bg-slate-950/20 px-3 text-xs text-slate-300 outline-none transition-all hover:border-slate-700 hover:bg-slate-950/40 focus:border-primary/60 cursor-pointer"
        >
          <span className="flex min-w-0 items-center gap-2">
            <span className={`h-2.5 w-2.5 shrink-0 rounded-full ${activeBackendTheme.dot}`} />
            <span className="truncate font-semibold tracking-wide">{backend.toUpperCase()}</span>
            <span className={`hidden text-[10px] font-medium sm:inline ${activeBackendTheme.text}`}>
              {backendStatusLabel(activeBackendStatus)}
            </span>
          </span>
          <ChevronDown
            className={`h-3.5 w-3.5 shrink-0 text-gray-400 transition-transform ${isBackendMenuOpen ? "rotate-180" : ""}`}
          />
        </button>

        {isBackendMenuOpen ? (
          <div
            role="listbox"
            aria-label="Backend"
            className="absolute bottom-10 left-0 z-30 w-[260px] overflow-hidden rounded-xl border border-slate-800 bg-app-surface/95 backdrop-blur-md p-1.5 shadow-2xl shadow-black/60"
          >
            {BACKENDS.map((item) => {
              const check = backendChecks[item];
              const status = backendStatus(check);
              const theme = backendStatusTheme(status);
              const isSelectedBackend = item === backend;
              return (
                <button
                  key={item}
                  type="button"
                  role="option"
                  aria-selected={isSelectedBackend}
                  onClick={() => {
                    setBackend(item);
                    setIsBackendMenuOpen(false);
                  }}
                  className={`flex w-full items-start gap-3 rounded-md border px-3 py-2.5 text-left transition-all ${theme.row} ${
                    isSelectedBackend ? "ring-1 ring-primary/45" : ""
                  }`}
                >
                  <span className={`mt-0.5 h-2.5 w-2.5 shrink-0 rounded-full ${theme.dot}`} />
                  <span className="min-w-0 flex-1">
                    <span className="flex items-center justify-between gap-3">
                      <span className="truncate text-xs font-bold tracking-wide text-gray-100">
                        {item.toUpperCase()}
                      </span>
                      <span className={`flex shrink-0 items-center gap-1 text-[10px] font-semibold ${theme.text}`}>
                        <BackendStatusIcon status={status} className={`h-3.5 w-3.5 ${theme.icon}`} />
                        {backendStatusLabel(status)}
                      </span>
                    </span>
                    <span className="mt-1 block truncate text-[11px] leading-4 text-gray-400" title={check?.details}>
                      {status === "checking" ? "Checking configuration" : check?.details || "Configured and reachable"}
                    </span>
                  </span>
                </button>
              );
            })}
          </div>
        ) : null}
      </div>
    </div>
  );

  return (
    <div
      className={`flex h-screen bg-app-canvas text-slate-100 font-sans ${
        isResizingInspector ? "cursor-col-resize select-none" : "select-none"
      }`}
    >
      <main className="flex min-w-0 flex-1 flex-col">
        {/* Header Bar */}
        <header className="flex items-center justify-between gap-4 border-b border-slate-800/50 bg-app-surface/80 backdrop-blur-md px-6 py-3 shrink-0">
          <div className="flex items-center gap-3">
            <div className="iota-logo-container relative flex h-8 w-8 items-center justify-center cursor-pointer transition-transform duration-300 hover:scale-105 active:scale-95">
              <svg
                viewBox="0 0 100 100"
                className="h-full w-full"
                fill="none"
                xmlns="http://www.w3.org/2000/svg"
              >
                <defs>
                  {/* Glow filters for futuristic look */}
                  <filter id="iota-glow" x="-20%" y="-20%" width="140%" height="140%">
                    <feGaussianBlur stdDeviation="4" result="blur" />
                    <feComposite in="SourceGraphic" in2="blur" operator="over" />
                  </filter>
                  {/* Vibrant Gradients */}
                  <linearGradient id="gradient-outer" x1="0%" y1="0%" x2="100%" y2="100%">
                    <stop offset="0%" stopColor="#FF6C37" />
                    <stop offset="50%" stopColor="#f97316" />
                    <stop offset="100%" stopColor="#f59e0b" />
                  </linearGradient>
                  <linearGradient id="gradient-inner" x1="100%" y1="0%" x2="0%" y2="100%">
                    <stop offset="0%" stopColor="#f59e0b" />
                    <stop offset="100%" stopColor="#FF6C37" />
                  </linearGradient>
                  <linearGradient id="gradient-core" x1="0%" y1="50%" x2="100%" y2="50%">
                    <stop offset="0%" stopColor="#FF6C37" />
                    <stop offset="100%" stopColor="#ef4444" />
                  </linearGradient>
                </defs>

                {/* Outer Orbit / Ring - Dashed for tech/cyberpunk dashboard look */}
                <circle
                  cx="50"
                  cy="50"
                  r="40"
                  stroke="url(#gradient-outer)"
                  strokeWidth="2.5"
                  strokeDasharray="12 8 4 8"
                  className="iota-logo-ring-outer opacity-70"
                />

                {/* Inner Orbit - Different dashes, counter-rotating */}
                <circle
                  cx="50"
                  cy="50"
                  r="30"
                  stroke="url(#gradient-inner)"
                  strokeWidth="1.5"
                  strokeDasharray="6 6"
                  className="iota-logo-ring-inner opacity-80"
                />

                {/* Nodes on the orbits (represents agents / nodes in orchestration) */}
                <circle
                  cx="50"
                  cy="10"
                  r="3"
                  fill="#ffdd57"
                  className="iota-logo-ring-outer"
                  filter="url(#iota-glow)"
                />
                <circle
                  cx="50"
                  cy="90"
                  r="3"
                  fill="#f43f5e"
                  className="iota-logo-ring-outer"
                  filter="url(#iota-glow)"
                />

                {/* Core glowing central sphere */}
                <circle
                  cx="50"
                  cy="50"
                  r="18"
                  fill="url(#gradient-core)"
                  className="iota-logo-core"
                  filter="url(#iota-glow)"
                  opacity="0.9"
                />

                {/* Elegant stylized letter "ι" in the center */}
                <path
                  d="M50 38V56C50 59 52 61 55 61"
                  stroke="white"
                  strokeWidth="3.5"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  className="iota-logo-core"
                />
                <circle
                  cx="50"
                  cy="31"
                  r="2"
                  fill="white"
                  className="iota-logo-core"
                />
              </svg>
            </div>
            <div>
              <h1 className="text-[15px] flex items-center gap-2">
                <span className="app-title-gradient font-extrabold tracking-wide">
                  Iota
                </span>
                <span className="text-primary font-semibold tracking-wide">
                  Desktop
                </span>
                <span className={`flex items-center gap-1.5 text-[10px] border px-2.5 py-0.5 rounded-full font-medium transition-all duration-300 backdrop-blur-sm ${statusTheme.badge}`}>
                  <span className="relative flex h-2 w-2">
                    <span className={`animate-ping absolute inline-flex h-full w-full rounded-full opacity-75 ${statusTheme.ping}`} />
                    <span className={`relative inline-flex rounded-full h-2 w-2 ${statusTheme.dot}`} />
                  </span>
                  {daemonStatusText}
                </span>
              </h1>
              <p className="text-[11px] text-slate-400 font-mono mt-0.5 flex items-center gap-1">
                <span className="text-slate-500">Model:</span>
                <span className="text-slate-300 font-medium">{modelName}</span>
                <span className={`text-[9px] px-1 rounded-sm ${isKeyConfigured ? "bg-emerald-500/10 text-emerald-400 border border-emerald-500/20" : "bg-rose-500/10 text-rose-400 border border-rose-500/20"}`}>
                  {isKeyConfigured ? "Key ✓" : "Key ✗"}
                </span>
              </p>
              <p className="text-[10px] text-slate-500 font-mono mt-0.5 truncate max-w-xs md:max-w-md" title={workspace}>
                {workspace || "Workspace unavailable"}
              </p>
            </div>
          </div>

          <button
            type="button"
            className="theme-toggle flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-slate-800 bg-slate-950/35 text-slate-400 transition-all hover:border-primary/40 hover:bg-primary/10 hover:text-primary"
            aria-label={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"}
            title={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"}
            onClick={() => setTheme((currentTheme) => currentTheme === "dark" ? "light" : "dark")}
          >
            {theme === "dark" ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
          </button>
        </header>

        {/* Content Panel */}
        {view === "chat" ? (
          <>
            {/* Messages Scroll Area */}
            <div className="flex-1 overflow-y-auto select-text bg-app-canvas">
              {transcript.length === 0 ? (
                <div className="flex h-full flex-col items-center justify-center text-slate-400 gap-3 px-6 text-center">
                  <div className="h-12 w-12 rounded-full bg-slate-900 border border-slate-800 flex items-center justify-center text-slate-500 shadow-inner">
                    <CheckCircle2 className="h-6 w-6 text-slate-400" />
                  </div>
                  <div className="max-w-md">
                    <p className="text-sm font-semibold text-slate-300">No prompt sent yet</p>
                    <p className="text-xs text-slate-500 mt-1">
                      {daemonError
                        ? daemonError
                        : selectedBackendReady
                          ? `Send a prompt to begin coding with ${backend.toUpperCase()}`
                          : `${backend.toUpperCase()} is not ready: ${activeBackendCheck?.details ?? "checking configuration"}`}
                    </p>
                  </div>
                </div>
              ) : (
                <div className="mx-auto max-w-3xl px-6 py-8 space-y-6">
                  {transcript.map((turn) => {
                    const isSelected = state.activeTurnId === turn.id;
                    return (
                      <div
                        key={turn.id}
                        onClick={() => dispatch({ type: "select_active_turn", turnId: turn.id })}
                        className={`p-4 rounded-xl cursor-pointer transition-all border ${
                          isSelected
                            ? "border-primary/30 bg-slate-900/35 shadow-md shadow-primary/5"
                            : "border-transparent hover:bg-slate-900/15"
                        }`}
                      >
                        <div className="mb-3.5 flex justify-end">
                          <div className="max-w-[85%] rounded-2xl rounded-tr-sm bg-primary/10 border border-primary/25 px-4 py-2.5 text-[13px] leading-relaxed text-slate-100 shadow-sm">
                            {turn.userPrompt}
                          </div>
                        </div>
                        <div className="flex flex-col items-start gap-1.5">
                          <span className="text-[10px] font-mono font-bold tracking-wider text-slate-500 px-1 uppercase">{turn.backend}</span>
                          <div className="w-full rounded-xl border border-slate-800/60 bg-slate-900/10 px-4.5 py-3.5 text-[13px] leading-relaxed text-slate-300 whitespace-pre-wrap font-sans">
                            {turn.assistantText ||
                              (turn.status === "failed" ? (
                                <span className="text-rose-400 font-medium">{turn.error}</span>
                              ) : turn.status === "queued" ? (
                                <span className="text-slate-500 italic">Queued...</span>
                              ) : (
                                <span className="text-blue-400 flex items-center gap-2 font-medium">
                                  <span className="h-2 w-2 rounded-full bg-blue-400 animate-pulse" />
                                  Running...
                                </span>
                              ))}
                          </div>
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>

            {/* Prompt Form */}
            <div className="border-t border-slate-800/40 bg-app-surface/60 backdrop-blur-md p-4 shrink-0">
              <div className="mb-3 flex justify-start">
                {workspaceControls}
              </div>
              <form onSubmit={onSubmit} className="mx-auto max-w-3xl">
                <div className="relative flex items-center bg-slate-950/40 border border-slate-800/80 rounded-xl px-3 py-2 shadow-inner focus-within:border-primary/60 transition-all">
                  <textarea
                    value={input}
                    onChange={(event) => setInput(event.target.value)}
                    rows={2}
                    className="min-h-[40px] flex-1 resize-none bg-transparent px-2.5 py-1 text-[13px] leading-relaxed text-slate-200 outline-none placeholder-slate-500 font-sans"
                    placeholder={
                      activeTurnBusy
                        ? "Wait for the active turn to finish or interrupt it..."
                        : daemonError
                          ? daemonError
                        : !selectedBackendReady
                          ? activeBackendCheck?.details ?? "Checking selected backend..."
                        : `Ask ${backend.toUpperCase()} to write code, debug, or solve tasks...`
                    }
                    onKeyDown={(event) => {
                      if (event.key === "Enter" && !event.shiftKey) {
                        event.preventDefault();
                        onSubmit(event);
                      }
                    }}
                  />
                  <button
                    type="submit"
                    disabled={activeTurnBusy || !!daemonError || !selectedBackendReady}
                    className="flex h-9 w-9 items-center justify-center rounded-lg bg-primary text-white hover:bg-primary-hover disabled:opacity-30 disabled:hover:bg-primary transition-all shadow-sm shadow-primary/20 cursor-pointer shrink-0 ml-2"
                  >
                    <Send className="h-4 w-4" />
                  </button>
                </div>
              </form>
            </div>
          </>
        ) : (
          <>
            <div className="min-h-0 flex-1">
              <ConfigPanel config={config} backendChecks={backendChecks} onConfigUpdate={handleConfigUpdate} />
            </div>
            <div className="border-t border-slate-800/40 bg-app-surface/60 backdrop-blur-md p-4 shrink-0">
              <div className="flex justify-start">
                {workspaceControls}
              </div>
            </div>
          </>
        )}
      </main>

      <div
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize inspector panel"
        className={`group relative z-10 w-2 shrink-0 cursor-col-resize bg-app-divider transition-colors ${
          isResizingInspector ? "bg-primary/20" : "hover:bg-primary/10"
        }`}
        onPointerDown={startInspectorResize}
      >
        <div
          className={`absolute left-1/2 top-0 h-full w-px -translate-x-1/2 transition-colors ${
            isResizingInspector ? "bg-primary" : "bg-slate-800 group-hover:bg-primary/80"
          }`}
        />
      </div>

      {/* Side Inspector Panel */}
      <RightInspector
        turn={activeTurn}
        observability={observability}
        width={inspectorWidth}
        activeTab={inspectorTab}
        onActiveTabChange={setInspectorTab}
        onApprovalDecision={(approvalId, approved) =>
          dispatch({ type: "approval_decision", approvalId, approved })
        }
      />
    </div>
  );
}
