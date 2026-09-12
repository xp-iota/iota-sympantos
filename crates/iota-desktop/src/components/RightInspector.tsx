import {
  AlertCircle,
  Clock,
  Terminal,
  Zap,
  Ban,
  Coins,
  Activity,
  ShieldAlert,
  Cpu,
  CheckCircle2,
  Database,
} from "lucide-react";
import * as React from "react";
import type { DesktopTurn, ObservabilitySummary } from "../types";
import { handleApproval, cancelTurn } from "../api";
import { MemoryContextWorkspace } from "./MemoryContextWorkspace";
import { KanbanWorkspace } from "./KanbanWorkspace";

type Props = {
  turn?: DesktopTurn;
  observability?: ObservabilitySummary | null;
  onApprovalDecision: (approvalId: string, approved: boolean) => void;
  width?: number;
  activeTab: InspectorTab;
  onActiveTabChange: (tab: InspectorTab) => void;
};

export type InspectorTab = "observability" | "kanban" | "memory" | "context";

export function RightInspector({ turn, observability, onApprovalDecision, width = 640, activeTab, onActiveTabChange }: Props) {
  const [cancelling, setCancelling] = React.useState(false);

  const formatMs = (ms?: number) => {
    if (ms === undefined) return "N/A";
    return `${(ms / 1000).toFixed(2)}s`;
  };

  const formatNumber = (num?: number) => {
    if (num === undefined) return "0";
    return new Intl.NumberFormat().format(num);
  };

  const renderObservabilitySection = () => (
    <section className="bg-slate-950/20 border border-slate-800/80 rounded-xl p-4 shadow-sm">
      <div className="flex items-center gap-2 text-xs font-semibold text-slate-400 uppercase tracking-wider mb-3">
        <Activity className="h-4 w-4 text-primary/80" />
        Recent Observability
      </div>
      {observability?.token_summary && observability.token_summary.length > 0 ? (
        <div className="space-y-2.5 text-xs text-slate-300">
          {observability.token_summary.slice(0, 5).map((summary) => (
            <div key={summary.backend} className="rounded-lg border border-slate-800 bg-app-canvas/40 p-3">
              <div className="flex items-center justify-between">
                <span className="font-bold uppercase text-slate-200">{summary.backend}</span>
                <span className="text-[10px] text-slate-500 font-mono font-medium">{summary.count} turns</span>
              </div>
              <div className="mt-2 grid grid-cols-3 gap-3 text-[10px] text-slate-500 font-mono">
                <div>
                  <span className="text-slate-600 block">Input</span>
                  <div className="text-slate-300 font-semibold mt-0.5">{formatNumber(Math.round(summary.input_tokens_mean ?? 0))}</div>
                </div>
                <div>
                  <span className="text-slate-600 block">Output</span>
                  <div className="text-slate-300 font-semibold mt-0.5">{formatNumber(Math.round(summary.output_tokens_mean ?? 0))}</div>
                </div>
                <div>
                  <span className="text-slate-600 block">Total</span>
                  <div className="text-primary font-semibold mt-0.5">{formatNumber(Math.round(summary.normalized_total_mean ?? 0))}</div>
                </div>
              </div>
            </div>
          ))}
          {observability.recent_token_executions && observability.recent_token_executions.length > 0 ? (
            <div className="border-t border-slate-800/60 pt-3">
              <div className="mb-2 text-[10px] font-bold uppercase tracking-wider text-slate-500">Recent Runs</div>
              <div className="space-y-2">
                {observability.recent_token_executions.slice(0, 3).map((execution) => (
                  <div key={execution.id} className="flex items-center justify-between text-xs text-slate-400">
                    <span className="truncate uppercase text-slate-500 font-medium">{execution.backend}</span>
                    <span className="font-mono text-slate-300 font-semibold bg-slate-950/30 px-2 py-0.5 rounded border border-slate-800/40 text-[11px]">
                      {formatNumber(execution.normalized_total_tokens)}
                    </span>
                  </div>
                ))}
              </div>
            </div>
          ) : null}
        </div>
      ) : (
        <div className="text-xs text-slate-500 italic">
          {observability?.error ?? "No recent token usage summary"}
        </div>
      )}
    </section>
  );

  const getStatusBadge = (status: string) => {
    switch (status) {
      case "queued":
        return <span className="rounded-md bg-gray-500/10 px-2.5 py-1 text-xs text-gray-400 border border-gray-500/10 font-medium">Queued</span>;
      case "running":
        return (
          <span className="flex items-center gap-1.5 rounded-md bg-blue-500/10 px-2.5 py-1 text-xs text-blue-400 border border-blue-500/20 font-medium animate-pulse">
            <span className="h-1.5 w-1.5 rounded-full bg-blue-400 animate-ping" />
            Running
          </span>
        );
      case "waiting_approval":
        return (
          <span className="flex items-center gap-1.5 rounded-md bg-amber-500/10 px-2.5 py-1 text-xs text-amber-450 border border-amber-500/20 font-medium">
            <ShieldAlert className="h-4 w-4" />
            Waiting Approval
          </span>
        );
      case "completed":
        return <span className="rounded-md bg-emerald-500/10 px-2.5 py-1 text-xs text-emerald-400 border border-emerald-500/20 font-medium">Completed</span>;
      case "failed":
        return <span className="rounded-md bg-rose-500/10 px-2.5 py-1 text-xs text-rose-450 border border-rose-500/20 font-medium">Failed</span>;
      case "cancelled":
        return <span className="rounded-md bg-slate-500/10 px-2.5 py-1 text-xs text-slate-400 border border-slate-500/20 font-medium">Cancelled</span>;
      default:
        return <span className="rounded-md bg-gray-500/10 px-2.5 py-1 text-xs text-gray-400 border border-gray-500/10 font-medium">{status}</span>;
    }
  };

  const handleInterrupt = async () => {
    if (!turn) return;
    setCancelling(true);
    try {
      await cancelTurn(turn.id);
    } catch (err) {
      console.error("Failed to cancel turn:", err);
    } finally {
      setCancelling(false);
    }
  };

  const renderTurnInspector = () => {
    if (!turn) {
      return (
        <section className="flex flex-col items-center justify-center gap-3 py-12 text-xs text-slate-500 border border-dashed border-slate-800/80 rounded-xl bg-slate-950/10">
          <Activity className="h-8 w-8 text-slate-700 animate-pulse" />
          <span>Select or start a turn to inspect</span>
        </section>
      );
    }

    const pendingApproval = turn.approvals.find((approval) => approval.status === "pending");

    return (
      <>
        <section className="border-b border-slate-800/60 pb-5">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2 text-[13px] font-bold text-slate-200">
              <Zap className="h-4.5 w-4.5 text-primary" />
              Active Turn Inspector
            </div>
            {getStatusBadge(turn.status)}
          </div>
          <div className="mt-4 space-y-2 text-xs text-slate-400 bg-slate-950/20 border border-slate-800/80 rounded-xl p-4">
            <div className="flex justify-between items-center">
              <span className="text-slate-500 font-medium">ID</span>
              <span className="font-mono text-slate-300 bg-slate-950/30 px-2 py-0.5 rounded border border-slate-800/40 text-[11px] font-semibold">{turn.id.slice(0, 8)}...</span>
            </div>
            <div className="flex justify-between items-center mt-1">
              <span className="text-slate-500 font-medium">Backend</span>
              <span className="font-bold text-slate-200 uppercase bg-slate-950/30 px-2 py-0.5 rounded border border-slate-800/40 text-[11px]">{turn.backend}</span>
            </div>
            {(turn.status === "running" || turn.status === "waiting_approval") && (
              <div className="mt-3 pt-3 border-t border-slate-800/40 flex justify-end">
                <button
                  disabled={cancelling}
                  onClick={handleInterrupt}
                  className="w-full flex items-center justify-center gap-2 rounded-lg bg-rose-500/10 hover:bg-rose-500/20 text-rose-300 disabled:opacity-50 py-2 text-xs font-semibold transition-colors border border-rose-500/20 cursor-pointer"
                >
                  <Ban className="h-3.5 w-3.5" />
                  {cancelling ? "Interrupting..." : "Interrupt Execution"}
                </button>
              </div>
            )}
          </div>
          <div className="mt-3">
            <button
              onClick={() => onActiveTabChange("memory")}
              className="w-full flex items-center justify-center gap-2 rounded-lg border border-slate-800 bg-slate-950/20 hover:bg-slate-950/40 text-slate-300 py-2 text-xs font-semibold transition-all cursor-pointer"
            >
              <Cpu className="h-3.5 w-3.5 text-primary" />
              Open Memory Workspace
            </button>
          </div>
        </section>

        {pendingApproval && (
          <section className="rounded-xl border border-rose-500/30 bg-rose-500/5 p-4 shadow-sm">
            <div className="flex items-center gap-2 text-xs font-bold text-rose-350">
              <AlertCircle className="h-4 w-4" />
              Approval Required
            </div>
            <p className="mt-2 text-xs text-slate-300 leading-normal">
              The agent requested execution of tool: <strong className="font-mono text-slate-100 bg-rose-950/30 px-1.5 py-0.5 rounded border border-rose-500/25">{pendingApproval.toolName}</strong>
            </p>
            <pre className="mt-2.5 max-h-36 overflow-auto rounded-lg bg-app-canvas p-3 text-[11px] text-slate-300 font-mono border border-slate-800">
              {JSON.stringify(pendingApproval.params, null, 2)}
            </pre>
            <div className="mt-4 flex justify-end gap-2.5">
              <button
                className="rounded-lg border border-slate-850 bg-slate-900/50 px-3.5 py-1.5 text-xs text-slate-300 hover:bg-slate-900 transition-colors cursor-pointer"
                onClick={async () => {
                  await handleApproval(pendingApproval.id, false);
                  onApprovalDecision(pendingApproval.id, false);
                }}
              >
                Deny
              </button>
              <button
                className="rounded-lg bg-primary px-4 py-1.5 text-xs text-white font-semibold hover:bg-primary-hover shadow-sm shadow-primary/25 transition-colors cursor-pointer"
                onClick={async () => {
                  await handleApproval(pendingApproval.id, true);
                  onApprovalDecision(pendingApproval.id, true);
                }}
              >
                Approve
              </button>
            </div>
          </section>
        )}

        <section className="bg-slate-950/20 border border-slate-800/80 rounded-xl p-4 shadow-sm">
          <div className="flex items-center gap-2 text-xs font-semibold text-slate-400 uppercase tracking-wider mb-3">
            <Clock className="h-4 w-4 text-primary/80" />
            Timing Summary
          </div>
          {turn.timing ? (
            <div className="grid grid-cols-2 gap-3 text-xs">
              <div className="bg-app-canvas/40 p-3 rounded-lg border border-slate-800">
                <div className="text-slate-500 font-medium">Total Duration</div>
                <div className="text-xs font-bold text-slate-200 mt-1 font-mono">{formatMs(turn.timing.total_ms)}</div>
              </div>
              <div className="bg-app-canvas/40 p-3 rounded-lg border border-slate-800">
                <div className="text-slate-500 font-medium">LLM Prompt</div>
                <div className="text-xs font-bold text-slate-200 mt-1 font-mono">{formatMs(turn.timing.prompt_ms)}</div>
              </div>
              <div className="bg-app-canvas/40 p-3 rounded-lg border border-slate-800">
                <div className="text-slate-500 font-medium">Spawn / Init</div>
                <div className="text-[11px] font-semibold text-slate-300 mt-1 truncate font-mono" title={`${formatMs(turn.timing.process_spawn_ms)} / ${formatMs(turn.timing.init_ms)}`}>
                  {formatMs(turn.timing.process_spawn_ms)} / {formatMs(turn.timing.init_ms)}
                </div>
              </div>
              <div className="bg-app-canvas/40 p-3 rounded-lg border border-slate-800">
                <div className="text-slate-500 font-medium">Session Mode</div>
                <div className="mt-1">
                  {turn.timing.session_reused ? (
                    <span className="rounded bg-emerald-500/10 px-2 py-0.5 text-[10px] text-emerald-400 font-bold border border-emerald-500/20">Reused</span>
                  ) : (
                    <span className="rounded bg-blue-500/10 px-2 py-0.5 text-[10px] text-blue-400 font-bold border border-blue-500/20">Cold Start</span>
                  )}
                </div>
              </div>
            </div>
          ) : (
            <div className="text-xs text-slate-500 italic">No timing recorded yet</div>
          )}
        </section>

        <section className="bg-slate-950/20 border border-slate-800/80 rounded-xl p-4 shadow-sm">
          <div className="flex items-center gap-2 text-xs font-semibold text-slate-400 uppercase tracking-wider mb-3">
            <Coins className="h-4 w-4 text-primary/80" />
            Token Usage
          </div>
          {turn.usage ? (
            <div className="space-y-2.5 text-xs text-slate-300">
              <div className="flex justify-between items-center">
                <span className="text-slate-500 font-medium">Input Tokens</span>
                <span className="font-bold font-mono text-slate-200">{formatNumber(turn.usage.input_tokens)}</span>
              </div>
              <div className="flex justify-between items-center">
                <span className="text-slate-500 font-medium">Output Tokens</span>
                <span className="font-bold font-mono text-slate-200">{formatNumber(turn.usage.output_tokens)}</span>
              </div>
              {(turn.usage.thinking_tokens ?? 0) > 0 && (
                <div className="flex justify-between items-center">
                  <span className="text-slate-500 font-medium">Thinking (Reasoning)</span>
                  <span className="font-semibold font-mono text-slate-400">{formatNumber(turn.usage.thinking_tokens)}</span>
                </div>
              )}
              {(turn.usage.cache_read_input_tokens ?? 0) > 0 && (
                <div className="flex justify-between items-center">
                  <span className="text-slate-500 font-medium">Cache Read Input</span>
                  <span className="font-bold font-mono text-emerald-400">{formatNumber(turn.usage.cache_read_input_tokens)}</span>
                </div>
              )}
              <div className="pt-2 border-t border-slate-800/80 flex justify-between items-center">
                <span className="font-bold text-slate-400">Total Tokens</span>
                <span className="font-extrabold font-mono text-[14px] text-primary">{formatNumber(turn.usage.total_tokens)}</span>
              </div>
            </div>
          ) : (
            <div className="text-xs text-slate-500 italic">No usage recorded yet</div>
          )}
        </section>

        <section className="flex flex-col gap-3">
          <div className="flex items-center gap-2 text-xs font-semibold text-slate-400 uppercase tracking-wider mb-1">
            <Terminal className="h-4 w-4 text-primary/85" />
            Tool Execution ({turn.toolCalls.length})
          </div>
          <div className="space-y-3 max-h-80 overflow-y-auto pr-1">
            {turn.toolCalls.length === 0 ? (
              <div className="text-xs text-slate-500 italic">No tools invoked in this turn</div>
            ) : null}
            {turn.toolCalls.map((tool) => (
              <div key={tool.id} className="rounded-xl border border-slate-800/80 bg-slate-950/20 overflow-hidden">
                <div className="flex items-center justify-between bg-slate-950/40 border-b border-slate-800/80 px-3.5 py-2.5 text-xs">
                  <div className="flex items-center gap-2 font-bold text-slate-200">
                    <Cpu className="h-3.5 w-3.5 text-primary" />
                    {tool.name}
                  </div>
                  {tool.ok === true ? (
                    <span className="flex items-center gap-1 rounded bg-emerald-500/10 px-2 py-0.5 text-[10px] text-emerald-400 font-bold border border-emerald-500/20">
                      <CheckCircle2 className="h-3 w-3" /> OK
                    </span>
                  ) : tool.ok === false ? (
                    <span className="flex items-center gap-1 rounded bg-rose-500/10 px-2 py-0.5 text-[10px] text-rose-400 font-bold border border-rose-500/20">
                      <AlertCircle className="h-3 w-3" /> Error
                    </span>
                  ) : (
                    <span className="rounded bg-amber-500/10 px-2 py-0.5 text-[10px] text-amber-400 font-bold border border-amber-500/20 animate-pulse">Running</span>
                  )}
                </div>
                <details className="text-xs text-slate-400">
                  <summary className="px-3.5 py-2 cursor-pointer text-[10px] text-slate-500 hover:text-slate-300 font-semibold select-none">
                    Parameters & Results
                  </summary>
                  <div className="p-3.5 bg-slate-950/30 space-y-3 font-mono">
                    <div>
                      <div className="text-[10px] text-slate-650 mb-1 uppercase font-bold tracking-wider">Arguments</div>
                      <pre className="text-xs bg-slate-950/60 rounded-lg p-3 overflow-x-auto text-slate-300 border border-slate-900">
                        {JSON.stringify(tool.arguments, null, 2)}
                      </pre>
                    </div>
                    {tool.result !== undefined && (
                      <div>
                        <div className="text-[10px] text-slate-650 mb-1 uppercase font-bold tracking-wider">Result</div>
                        <pre className="text-xs bg-slate-950/60 rounded-lg p-3 overflow-x-auto text-slate-300 max-h-48 overflow-y-auto border border-slate-900">
                          {JSON.stringify(tool.result, null, 2)}
                        </pre>
                      </div>
                    )}
                  </div>
                </details>
              </div>
            ))}
          </div>
        </section>

        <section className="flex flex-col gap-3">
          <div className="flex items-center gap-2 text-xs font-semibold text-slate-400 uppercase tracking-wider mb-1">
            <Activity className="h-4 w-4 text-primary/85" />
            Runtime Events ({turn.events.length})
          </div>
          <div className="space-y-3 max-h-80 overflow-y-auto pr-1">
            {turn.events.length === 0 ? (
              <div className="text-xs text-slate-500 italic">No events generated yet</div>
            ) : null}
            {turn.events.map((event, index) => (
              <details key={index} className="rounded-lg border border-slate-800 bg-app-canvas/30 text-xs overflow-hidden">
                <summary className="px-3.5 py-2.5 cursor-pointer font-bold text-slate-300 hover:bg-app-canvas/60 flex justify-between items-center select-none">
                  <span>{event.kind}</span>
                  <span className="text-[10px] text-slate-500 font-mono font-medium">#{index + 1}</span>
                </summary>
                <pre className="p-3.5 bg-slate-950/50 border-t border-slate-900 font-mono text-[11px] text-slate-400 overflow-x-auto max-h-56 overflow-y-auto">
                  {JSON.stringify(event.data, null, 2)}
                </pre>
              </details>
            ))}
          </div>
        </section>
      </>
    );
  };

  return (
    <aside
      className="shrink-0 border-l border-slate-800 bg-app-surface overflow-hidden flex flex-col"
      style={{ width }}
    >
      <div className="border-b border-slate-800 bg-app-surface p-3">
        <nav className="grid grid-cols-4 rounded-lg border border-slate-800 bg-slate-950/40 p-0.5">
          <button
            className={`flex items-center justify-center gap-1.5 rounded-md px-2 py-1.5 text-xs font-semibold transition-all cursor-pointer ${
              activeTab === "observability" ? "bg-primary text-white shadow-sm shadow-primary/20 border border-primary/10" : "text-slate-400 hover:text-slate-200 border border-transparent"
            }`}
            onClick={() => onActiveTabChange("observability")}
          >
            <Activity className="h-3.5 w-3.5" />
            Observability
          </button>
          <button
            className={`flex items-center justify-center gap-1.5 rounded-md px-2 py-1.5 text-xs font-semibold transition-all cursor-pointer ${
              activeTab === "kanban" ? "bg-primary text-white shadow-sm shadow-primary/20 border border-primary/10" : "text-slate-400 hover:text-slate-200 border border-transparent"
            }`}
            onClick={() => onActiveTabChange("kanban")}
          >
            <Database className="h-3.5 w-3.5" />
            Kanban
          </button>
          <button
            className={`flex items-center justify-center gap-1.5 rounded-md px-2 py-1.5 text-xs font-semibold transition-all cursor-pointer ${
              activeTab === "memory" ? "bg-primary text-white shadow-sm shadow-primary/20 border border-primary/10" : "text-slate-400 hover:text-slate-200 border border-transparent"
            }`}
            onClick={() => onActiveTabChange("memory")}
          >
            <Database className="h-3.5 w-3.5" />
            Memory
          </button>
          <button
            className={`flex items-center justify-center gap-1.5 rounded-md px-2 py-1.5 text-xs font-semibold transition-all cursor-pointer ${
              activeTab === "context" ? "bg-primary text-white shadow-sm shadow-primary/20 border border-primary/10" : "text-slate-400 hover:text-slate-200 border border-transparent"
            }`}
            onClick={() => onActiveTabChange("context")}
          >
            <Cpu className="h-3.5 w-3.5" />
            Context
          </button>
        </nav>
      </div>
      {activeTab === "observability" ? (
        <div className="flex flex-col gap-6 overflow-y-auto p-5">
          {renderTurnInspector()}
          {renderObservabilitySection()}
        </div>
      ) : activeTab === "kanban" ? (
        <div className="min-h-0 flex-1 overflow-hidden">
          <KanbanWorkspace />
        </div>
      ) : activeTab === "memory" ? (
        <div className="min-h-0 flex-1 overflow-hidden">
          <MemoryContextWorkspace mode="memory" />
        </div>
      ) : (
        <div className="min-h-0 flex-1 overflow-hidden">
          <MemoryContextWorkspace mode="context" />
        </div>
      )}
    </aside>
  );
}
