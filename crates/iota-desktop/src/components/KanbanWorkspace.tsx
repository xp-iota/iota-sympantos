import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Activity,
  AlertCircle,
  GitBranch,
  Link2,
  MessageSquare,
  Play,
  Plus,
  RefreshCw,
  Search,
  X,
} from "lucide-react";
import {
  addKanbanComment,
  createKanbanLink,
  createKanbanTask,
  dispatchKanban,
  getKanbanTaskDetail,
  listenKanbanUpdates,
  listKanbanBoards,
  listKanbanTasks,
  removeKanbanLink,
  transitionKanbanTask,
  updateKanbanTask,
} from "../api";
import type { KanbanBoard, KanbanDispatchReport, KanbanEvent, KanbanLinkKind, KanbanStatus, KanbanTask, KanbanTaskDetail } from "../types";
import {
  BOARD_STATUSES,
  STATUS_LABELS,
  buildKanbanColumns,
  filterKanbanTasks,
  formatDispatchReport,
  groupRunningByLane,
  legalStatusActions,
  uniqueAssignees,
} from "./kanbanModel";

const STATUS_STYLES: Record<KanbanStatus, string> = {
  triage: "border-slate-700/70 bg-slate-900/30 text-slate-300",
  todo: "border-sky-500/25 bg-sky-500/10 text-sky-200",
  ready: "border-amber-500/25 bg-amber-500/10 text-amber-200",
  running: "border-blue-500/25 bg-blue-500/10 text-blue-200",
  blocked: "border-rose-500/25 bg-rose-500/10 text-rose-200",
  done: "border-emerald-500/25 bg-emerald-500/10 text-emerald-200",
  archived: "border-slate-700 bg-slate-950/30 text-slate-500",
};

function formatBoardName(task: KanbanTask, boards: KanbanBoard[]) {
  return boards.find((board) => board.id === task.board_id)?.slug ?? `board-${task.board_id}`;
}

function formatUnixSeconds(value?: number) {
  if (!value) return "-";
  return new Date(value * 1000).toLocaleString();
}

function formatEventPayload(event: KanbanEvent) {
  try {
    return JSON.stringify(JSON.parse(event.payload), null, 2);
  } catch {
    return event.payload;
  }
}

export function KanbanWorkspace() {
  const [boards, setBoards] = useState<KanbanBoard[]>([]);
  const [tasks, setTasks] = useState<KanbanTask[]>([]);
  const [search, setSearch] = useState("");
  const [selectedBoardId, setSelectedBoardId] = useState<number | "all">("all");
  const [selectedAssignee, setSelectedAssignee] = useState<string | "all">("all");
  const [lanesByProfile, setLanesByProfile] = useState(true);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [dispatching, setDispatching] = useState(false);
  const [lastReport, setLastReport] = useState<KanbanDispatchReport | null>(null);
  const [lastDispatchAt, setLastDispatchAt] = useState<Date | null>(null);
  const [selectedTaskId, setSelectedTaskId] = useState<number | null>(null);
  const [detail, setDetail] = useState<KanbanTaskDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [commentBody, setCommentBody] = useState("");
  const [editTitle, setEditTitle] = useState("");
  const [editBody, setEditBody] = useState("");
  const [editAssignee, setEditAssignee] = useState("");
  const [editPriority, setEditPriority] = useState("0");
  const [editTags, setEditTags] = useState("");
  const [linkTarget, setLinkTarget] = useState("");
  const [linkKind, setLinkKind] = useState<KanbanLinkKind>("related");
  const [taskActionPending, setTaskActionPending] = useState(false);
  const [creatingTask, setCreatingTask] = useState(false);
  const [newTaskTitle, setNewTaskTitle] = useState("");
  const [newTaskBody, setNewTaskBody] = useState("");
  const [newTaskAssignee, setNewTaskAssignee] = useState("");
  const [newTaskStatus, setNewTaskStatus] = useState<KanbanStatus>("ready");
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async (options: { tick?: boolean; showSpinner?: boolean } = {}) => {
    if (options.showSpinner) {
      setRefreshing(true);
      setLoading(true);
    }
    try {
      setError(null);
      if (options.tick) {
        setLastReport(await dispatchKanban());
        setLastDispatchAt(new Date());
      }
      const [nextBoards, nextTasks] = await Promise.all([listKanbanBoards(), listKanbanTasks({ limit: 200 })]);
      setBoards(nextBoards);
      setTasks(nextTasks);
      if (selectedTaskId !== null) {
        setDetail(await getKanbanTaskDetail(selectedTaskId));
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  }, [selectedTaskId]);

  useEffect(() => {
    refresh();
    const interval = window.setInterval(refresh, 3000);
    return () => window.clearInterval(interval);
  }, [refresh]);

  useEffect(() => {
    let disposed = false;
    let cleanup: (() => void) | undefined;
    listenKanbanUpdates((report) => {
      if (disposed) return;
      setLastReport(report);
      setLastDispatchAt(new Date());
      refresh();
    })
      .then((unlisten) => {
        if (disposed) {
          unlisten();
        } else {
          cleanup = unlisten;
        }
      })
      .catch((err) => setError(err instanceof Error ? err.message : String(err)));
    return () => {
      disposed = true;
      cleanup?.();
    };
  }, [refresh]);

  useEffect(() => {
    if (selectedTaskId === null) {
      setDetail(null);
      return;
    }
    let cancelled = false;
    setDetailLoading(true);
    getKanbanTaskDetail(selectedTaskId)
      .then((nextDetail) => {
        if (!cancelled) setDetail(nextDetail);
      })
      .catch((err) => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (!cancelled) setDetailLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedTaskId]);

  useEffect(() => {
    if (!detail) return;
    setEditTitle(detail.task.title);
    setEditBody(detail.task.body ?? "");
    setEditAssignee(detail.task.assignee ?? "");
    setEditPriority(String(detail.task.priority));
    setEditTags(detail.task.tags.join(", "));
  }, [detail]);

  const filteredTasks = useMemo(
    () => filterKanbanTasks(tasks, { search, assignee: selectedAssignee, boardId: selectedBoardId }),
    [search, selectedAssignee, selectedBoardId, tasks],
  );
  const columns = useMemo(() => buildKanbanColumns(filteredTasks), [filteredTasks]);
  const assignees = useMemo(() => uniqueAssignees(tasks), [tasks]);

  const runDispatch = async () => {
    setDispatching(true);
    try {
      setLastReport(await dispatchKanban());
      setLastDispatchAt(new Date());
      await refresh({ showSpinner: true });
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setDispatching(false);
    }
  };

  const changeTaskStatus = async (toStatus: KanbanStatus) => {
    if (selectedTaskId === null) return;
    setTaskActionPending(true);
    try {
      await transitionKanbanTask(selectedTaskId, toStatus);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setTaskActionPending(false);
    }
  };

  const submitComment = async () => {
    if (selectedTaskId === null || !commentBody.trim()) return;
    setTaskActionPending(true);
    try {
      await addKanbanComment(selectedTaskId, "desktop", commentBody.trim());
      setCommentBody("");
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setTaskActionPending(false);
    }
  };

  const saveTaskEdits = async () => {
    if (selectedTaskId === null || !editTitle.trim()) return;
    setTaskActionPending(true);
    try {
      await updateKanbanTask(selectedTaskId, {
        title: editTitle.trim(),
        body: editBody.trim() || null,
        assignee: editAssignee.trim() || null,
        priority: Number.parseInt(editPriority, 10) || 0,
        tags: editTags
          .split(",")
          .map((tag) => tag.trim())
          .filter(Boolean),
      });
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setTaskActionPending(false);
    }
  };

  const addLink = async () => {
    if (selectedTaskId === null) return;
    const toId = Number.parseInt(linkTarget, 10);
    if (!Number.isFinite(toId) || toId <= 0) return;
    setTaskActionPending(true);
    try {
      await createKanbanLink({ from_id: selectedTaskId, to_id: toId, kind: linkKind });
      setLinkTarget("");
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setTaskActionPending(false);
    }
  };

  const deleteLink = async (fromId: number, toId: number, kind: KanbanLinkKind) => {
    setTaskActionPending(true);
    try {
      await removeKanbanLink({ from_id: fromId, to_id: toId, kind });
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setTaskActionPending(false);
    }
  };

  const submitNewTask = async () => {
    const boardId = selectedBoardId === "all" ? boards[0]?.id : selectedBoardId;
    if (!boardId || !newTaskTitle.trim()) return;
    setTaskActionPending(true);
    try {
      const taskId = await createKanbanTask({
        board_id: boardId,
        title: newTaskTitle.trim(),
        body: newTaskBody.trim() || null,
        status: newTaskStatus,
        assignee: newTaskAssignee.trim() || null,
        priority: 0,
        tags: [],
        workspace_kind: null,
        workspace_path: null,
      });
      setCreatingTask(false);
      setNewTaskTitle("");
      setNewTaskBody("");
      setNewTaskAssignee("");
      setSelectedTaskId(taskId);
      await refresh({ tick: newTaskStatus === "ready" });
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setTaskActionPending(false);
    }
  };

  const renderTaskCard = (task: KanbanTask) => (
    <article
      key={task.id}
      onClick={() => setSelectedTaskId(task.id)}
      className={`cursor-pointer rounded-lg border bg-slate-950/30 p-3 transition-colors ${
        selectedTaskId === task.id ? "border-primary/50 ring-1 ring-primary/25" : "border-slate-800/80 hover:border-slate-700"
      }`}
    >
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <div className="flex items-center gap-1.5 text-[10px] font-mono text-slate-500">
            <span>#{task.id}</span>
            <span>{formatBoardName(task, boards)}</span>
          </div>
          <h3 className="mt-1 line-clamp-2 text-xs font-bold leading-snug text-slate-100">{task.title}</h3>
        </div>
        <span className={`shrink-0 rounded-md border px-1.5 py-0.5 text-[9px] font-bold ${STATUS_STYLES[task.status]}`}>
          {STATUS_LABELS[task.status]}
        </span>
      </div>
      {task.body ? <p className="mt-2 line-clamp-3 text-[11px] leading-relaxed text-slate-400">{task.body}</p> : null}
      <div className="mt-2 flex flex-wrap items-center gap-1.5 text-[10px] text-slate-500">
        <span className="rounded border border-slate-800 bg-slate-950/30 px-1.5 py-0.5">{task.assignee || "default"}</span>
        <span className="rounded border border-slate-800 bg-slate-950/30 px-1.5 py-0.5">P{task.priority}</span>
        {task.tags.slice(0, 3).map((tag) => (
          <span key={tag} className="rounded border border-slate-800 bg-slate-950/30 px-1.5 py-0.5">{tag}</span>
        ))}
      </div>
    </article>
  );

  return (
    <div className="flex h-full flex-col bg-app-surface">
      <div className="border-b border-slate-800/80 p-4">
        <div className="flex items-start justify-between gap-3">
          <div>
            <div className="flex items-center gap-2 text-[13px] font-bold text-slate-200">
              <Activity className="h-4 w-4 text-primary" />
              Hermes Kanban
            </div>
            <p className="mt-1 text-[11px] text-slate-500">{formatDispatchReport(lastReport)}</p>
            {lastDispatchAt ? (
              <p className="mt-0.5 text-[10px] text-slate-600">Last tick {lastDispatchAt.toLocaleTimeString()}</p>
            ) : null}
          </div>
          <div className="flex flex-wrap justify-end gap-2">
            <button
              type="button"
              onClick={() => setCreatingTask((value) => !value)}
              className="flex h-8 items-center gap-1.5 rounded-lg border border-slate-800 bg-slate-950/30 px-3 text-xs font-semibold text-slate-200 transition-colors hover:bg-slate-950/50"
            >
              <Plus className="h-3.5 w-3.5" />
              New task
            </button>
            <button
              type="button"
              onClick={() => refresh({ tick: true, showSpinner: true })}
              disabled={refreshing}
              className="flex h-8 w-8 items-center justify-center rounded-lg border border-slate-800 bg-slate-950/25 text-slate-300 transition-colors hover:bg-slate-950/45"
              aria-label="Refresh kanban tasks"
            >
              <RefreshCw className={`h-3.5 w-3.5 ${loading || refreshing ? "animate-spin" : ""}`} />
            </button>
            <button
              type="button"
              onClick={runDispatch}
              disabled={dispatching}
              className="flex h-8 items-center gap-1.5 rounded-lg bg-primary px-3 text-xs font-semibold text-white shadow-sm shadow-primary/20 transition-colors hover:bg-primary-hover disabled:opacity-50"
            >
              <Play className="h-3.5 w-3.5" />
              Nudge dispatcher
            </button>
          </div>
        </div>

        <div className="mt-4 grid grid-cols-1 gap-2 xl:grid-cols-[minmax(180px,1fr)_140px_140px_auto]">
          <label className="flex h-8 items-center gap-2 rounded-lg border border-slate-800 bg-slate-950/30 px-2 text-slate-500">
            <Search className="h-3.5 w-3.5" />
            <input
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              placeholder="Search tasks"
              className="min-w-0 flex-1 bg-transparent text-xs text-slate-200 outline-none placeholder:text-slate-600"
            />
          </label>
          <select
            value={selectedBoardId}
            onChange={(event) => setSelectedBoardId(event.target.value === "all" ? "all" : Number(event.target.value))}
            className="h-8 rounded-lg border border-slate-800 bg-slate-950/30 px-2 text-xs text-slate-300 outline-none"
          >
            <option value="all">All boards</option>
            {boards.map((board) => (
              <option key={board.id} value={board.id}>{board.slug}</option>
            ))}
          </select>
          <select
            value={selectedAssignee}
            onChange={(event) => setSelectedAssignee(event.target.value)}
            className="h-8 rounded-lg border border-slate-800 bg-slate-950/30 px-2 text-xs text-slate-300 outline-none"
          >
            <option value="all">All lanes</option>
            {assignees.map((assignee) => (
              <option key={assignee} value={assignee}>{assignee}</option>
            ))}
          </select>
          <label className="flex h-8 items-center gap-2 rounded-lg border border-slate-800 bg-slate-950/30 px-2 text-[11px] font-semibold text-slate-300">
            <input type="checkbox" checked={lanesByProfile} onChange={(event) => setLanesByProfile(event.target.checked)} />
            Lanes by profile
          </label>
        </div>
      </div>

      {creatingTask ? (
        <div className="border-b border-slate-800/80 bg-slate-950/20 p-4">
          <div className="grid gap-2">
            <input
              value={newTaskTitle}
              onChange={(event) => setNewTaskTitle(event.target.value)}
              placeholder="Task title"
              className="h-9 rounded-lg border border-slate-800 bg-slate-950/30 px-3 text-sm text-slate-100 outline-none placeholder:text-slate-600"
            />
            <textarea
              value={newTaskBody}
              onChange={(event) => setNewTaskBody(event.target.value)}
              placeholder="Task body"
              className="min-h-20 resize-none rounded-lg border border-slate-800 bg-slate-950/30 px-3 py-2 text-xs text-slate-200 outline-none placeholder:text-slate-600"
            />
            <div className="grid grid-cols-1 gap-2 md:grid-cols-[1fr_140px_auto]">
              <input
                value={newTaskAssignee}
                onChange={(event) => setNewTaskAssignee(event.target.value)}
                placeholder="Assignee / profile"
                className="h-8 rounded-lg border border-slate-800 bg-slate-950/30 px-3 text-xs text-slate-200 outline-none placeholder:text-slate-600"
              />
              <select
                value={newTaskStatus}
                onChange={(event) => setNewTaskStatus(event.target.value as KanbanStatus)}
                className="h-8 rounded-lg border border-slate-800 bg-slate-950/30 px-2 text-xs text-slate-300 outline-none"
              >
                {BOARD_STATUSES.map((status) => (
                  <option key={status} value={status}>{STATUS_LABELS[status]}</option>
                ))}
              </select>
              <div className="flex gap-2">
                <button
                  type="button"
                  onClick={submitNewTask}
                  disabled={taskActionPending || !newTaskTitle.trim()}
                  className="h-8 rounded-lg bg-primary px-3 text-xs font-semibold text-white disabled:opacity-50"
                >
                  Create
                </button>
                <button
                  type="button"
                  onClick={() => setCreatingTask(false)}
                  className="h-8 rounded-lg border border-slate-800 bg-slate-950/30 px-3 text-xs font-semibold text-slate-300"
                >
                  Cancel
                </button>
              </div>
            </div>
          </div>
        </div>
      ) : null}

      <div className="flex min-h-0 flex-1">
        <main className="min-w-0 flex-1 overflow-auto p-4">
          {error ? (
            <div className="mb-3 flex items-start gap-2 rounded-lg border border-rose-500/25 bg-rose-500/10 p-3 text-xs text-rose-200">
              <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
              <span>{error}</span>
            </div>
          ) : null}
          {loading && tasks.length === 0 ? (
            <div className="rounded-lg border border-dashed border-slate-800 bg-slate-950/10 p-8 text-center text-xs text-slate-500">
              Loading kanban tasks...
            </div>
          ) : (
            <div className="grid min-w-[1080px] grid-cols-6 gap-3">
              {columns.map((column) => (
                <section key={column.status} className="min-h-[320px] rounded-lg border border-slate-800/80 bg-slate-950/20">
                  <div className="flex items-center justify-between border-b border-slate-800/70 px-3 py-2">
                    <span className="text-[11px] font-bold uppercase tracking-wide text-slate-300">{column.label}</span>
                    <span className="rounded border border-slate-800 px-1.5 py-0.5 text-[10px] text-slate-500">{column.tasks.length}</span>
                  </div>
                  <div className="space-y-2 p-2">
                    {column.status === "running" && lanesByProfile ? (
                      groupRunningByLane(column.tasks).length === 0 ? (
                        <div className="rounded-lg border border-dashed border-slate-800 p-3 text-center text-[11px] text-slate-600">Empty</div>
                      ) : (
                        groupRunningByLane(column.tasks).map((group) => (
                          <div key={group.lane} className="space-y-2">
                            <div className="flex items-center gap-1 px-1 text-[10px] font-bold uppercase tracking-wide text-slate-500">
                              <GitBranch className="h-3 w-3" />
                              {group.lane} · {group.tasks.length}
                            </div>
                            {group.tasks.map((task) => renderTaskCard(task))}
                          </div>
                        ))
                      )
                    ) : column.tasks.length === 0 ? (
                      <div className="rounded-lg border border-dashed border-slate-800 p-3 text-center text-[11px] text-slate-600">Empty</div>
                    ) : (
                      column.tasks.map((task) => renderTaskCard(task))
                    )}
                  </div>
                </section>
              ))}
            </div>
          )}
        </main>

        {selectedTaskId !== null ? (
          <aside className="flex w-[390px] shrink-0 flex-col border-l border-slate-800/80 bg-app-surface-raised">
            <div className="flex items-start justify-between gap-3 border-b border-slate-800/80 p-4">
              <div className="min-w-0">
                <div className="text-[10px] font-bold uppercase tracking-wider text-primary/90">Task Drawer</div>
                <h3 className="mt-1 truncate text-sm font-bold text-slate-100">{detail?.task.title ?? `#${selectedTaskId}`}</h3>
                <p className="mt-1 text-[11px] text-slate-500">{detailLoading ? "Loading detail..." : detail?.board?.slug ?? "iota kanban db"}</p>
              </div>
              <button
                type="button"
                onClick={() => setSelectedTaskId(null)}
                className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg border border-slate-800 bg-slate-950/25 text-slate-400 hover:text-slate-100"
                aria-label="Close task drawer"
              >
                <X className="h-3.5 w-3.5" />
              </button>
            </div>

            <div className="min-h-0 flex-1 overflow-y-auto p-4">
              {detail ? (
                <div className="space-y-4">
                  <div className="flex flex-wrap gap-2">
                    {legalStatusActions(detail.task.status).map((action) => (
                      <button
                        key={action.to}
                        type="button"
                        onClick={() => changeTaskStatus(action.to)}
                        disabled={taskActionPending}
                        className="rounded-lg border border-primary/30 bg-primary/10 px-2.5 py-1.5 text-[11px] font-semibold text-primary hover:bg-primary/15 disabled:opacity-50"
                      >
                        {action.label}
                      </button>
                    ))}
                  </div>

                  {detail.task.body ? <p className="text-xs leading-relaxed text-slate-300">{detail.task.body}</p> : null}

                  <div className="space-y-2 rounded-lg border border-slate-800 bg-slate-950/20 p-3">
                    <div className="text-[10px] font-bold uppercase tracking-wider text-slate-500">Edit Task</div>
                    <input
                      value={editTitle}
                      onChange={(event) => setEditTitle(event.target.value)}
                      className="h-8 w-full rounded-lg border border-slate-800 bg-slate-950/30 px-2 text-xs text-slate-100 outline-none"
                    />
                    <textarea
                      value={editBody}
                      onChange={(event) => setEditBody(event.target.value)}
                      className="min-h-16 w-full resize-none rounded-lg border border-slate-800 bg-slate-950/30 px-2 py-1.5 text-xs text-slate-200 outline-none"
                    />
                    <div className="grid grid-cols-[1fr_70px] gap-2">
                      <input
                        value={editAssignee}
                        onChange={(event) => setEditAssignee(event.target.value)}
                        placeholder="Assignee"
                        className="h-8 rounded-lg border border-slate-800 bg-slate-950/30 px-2 text-xs text-slate-200 outline-none placeholder:text-slate-600"
                      />
                      <input
                        value={editPriority}
                        onChange={(event) => setEditPriority(event.target.value)}
                        placeholder="P0"
                        className="h-8 rounded-lg border border-slate-800 bg-slate-950/30 px-2 text-xs text-slate-200 outline-none placeholder:text-slate-600"
                      />
                    </div>
                    <input
                      value={editTags}
                      onChange={(event) => setEditTags(event.target.value)}
                      placeholder="tags, comma, separated"
                      className="h-8 w-full rounded-lg border border-slate-800 bg-slate-950/30 px-2 text-xs text-slate-200 outline-none placeholder:text-slate-600"
                    />
                    <button
                      type="button"
                      onClick={saveTaskEdits}
                      disabled={taskActionPending || !editTitle.trim()}
                      className="h-8 rounded-lg bg-primary px-3 text-xs font-semibold text-white disabled:opacity-50"
                    >
                      Save changes
                    </button>
                  </div>

                  <div className="grid grid-cols-2 gap-2 text-[11px] text-slate-400">
                    <div className="rounded-lg border border-slate-800 bg-slate-950/25 p-2">
                      <span className="block text-slate-600">Created</span>
                      <span className="font-mono text-slate-300">{formatUnixSeconds(detail.task.created_at)}</span>
                    </div>
                    <div className="rounded-lg border border-slate-800 bg-slate-950/25 p-2">
                      <span className="block text-slate-600">Updated</span>
                      <span className="font-mono text-slate-300">{formatUnixSeconds(detail.task.updated_at)}</span>
                    </div>
                  </div>

                  <div className="grid grid-cols-3 gap-2 text-[11px]">
                    <div className="rounded-lg border border-slate-800 bg-slate-950/25 p-2 text-slate-300">
                      <span className="block text-slate-600">Runs</span>
                      {detail.runs.length}
                    </div>
                    <div className="rounded-lg border border-slate-800 bg-slate-950/25 p-2 text-slate-300">
                      <span className="block text-slate-600">Comments</span>
                      {detail.comments.length}
                    </div>
                    <div className="rounded-lg border border-slate-800 bg-slate-950/25 p-2 text-slate-300">
                      <span className="block text-slate-600">Links</span>
                      {detail.links.length}
                    </div>
                  </div>

                  <div className="space-y-2">
                    <div className="flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-wider text-slate-500">
                      <Activity className="h-3 w-3 text-primary/80" /> Run History
                    </div>
                    {detail.runs.length === 0 ? (
                      <div className="text-xs text-slate-500">No runs recorded</div>
                    ) : detail.runs.map((run) => (
                      <div key={run.id} className="rounded-lg border border-slate-800 bg-slate-950/25 p-2 text-[11px] text-slate-400">
                        <div className="flex items-center justify-between gap-2">
                          <span className="truncate font-mono text-slate-300">{run.id}</span>
                          <span className="shrink-0 rounded border border-slate-700 px-1.5 py-0.5 text-[10px] uppercase text-slate-300">{run.status}</span>
                        </div>
                        <div className="mt-1 font-mono text-[10px] text-slate-500">{run.profile} · {formatUnixSeconds(run.started_at)}</div>
                        {run.output_summary ? <p className="mt-1 text-slate-400">{run.output_summary}</p> : null}
                      </div>
                    ))}
                  </div>

                  <div className="space-y-2">
                    <div className="flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-wider text-slate-500">
                      <MessageSquare className="h-3 w-3 text-primary/80" /> Comments
                    </div>
                    {detail.comments.length === 0 ? (
                      <div className="text-xs text-slate-500">No comments recorded</div>
                    ) : detail.comments.map((comment) => (
                      <div key={comment.id} className="rounded-lg border border-slate-800 bg-slate-950/25 p-2 text-[11px] text-slate-400">
                        <div className="font-semibold text-slate-300">{comment.author}</div>
                        <p className="mt-1 leading-relaxed">{comment.body}</p>
                      </div>
                    ))}
                    <div className="mt-2 flex gap-2">
                      <input
                        value={commentBody}
                        onChange={(event) => setCommentBody(event.target.value)}
                        placeholder="Add comment"
                        className="h-8 min-w-0 flex-1 rounded-lg border border-slate-800 bg-slate-950/30 px-2 text-xs text-slate-200 outline-none placeholder:text-slate-600"
                      />
                      <button
                        type="button"
                        onClick={submitComment}
                        disabled={taskActionPending || !commentBody.trim()}
                        className="h-8 rounded-lg bg-primary px-3 text-xs font-semibold text-white disabled:opacity-50"
                      >
                        Add
                      </button>
                    </div>
                  </div>

                  <div className="space-y-2">
                    <div className="flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-wider text-slate-500">
                      <Link2 className="h-3 w-3 text-primary/80" /> Links & Events
                    </div>
                    {detail.links.length > 0 ? (
                      <div className="space-y-1.5">
                        {detail.links.map((link) => (
                          <div key={`${link.from_id}-${link.to_id}-${link.kind}`} className="flex items-center justify-between gap-2 rounded border border-slate-800 bg-slate-950/25 px-2 py-1 text-[10px] text-slate-400">
                            <span>#{link.from_id} {link.kind} #{link.to_id}</span>
                            <button
                              type="button"
                              onClick={() => deleteLink(link.from_id, link.to_id, link.kind)}
                              disabled={taskActionPending}
                              className="text-slate-500 hover:text-rose-200 disabled:opacity-50"
                            >
                              Remove
                            </button>
                          </div>
                        ))}
                      </div>
                    ) : null}
                    <div className="grid grid-cols-[1fr_95px_auto] gap-2">
                      <input
                        value={linkTarget}
                        onChange={(event) => setLinkTarget(event.target.value)}
                        placeholder="Link task id"
                        className="h-8 rounded-lg border border-slate-800 bg-slate-950/30 px-2 text-xs text-slate-200 outline-none placeholder:text-slate-600"
                      />
                      <select
                        value={linkKind}
                        onChange={(event) => setLinkKind(event.target.value as KanbanLinkKind)}
                        className="h-8 rounded-lg border border-slate-800 bg-slate-950/30 px-2 text-xs text-slate-300 outline-none"
                      >
                        <option value="related">related</option>
                        <option value="parent">parent</option>
                        <option value="blocks">blocks</option>
                      </select>
                      <button
                        type="button"
                        onClick={addLink}
                        disabled={taskActionPending || !linkTarget.trim()}
                        className="h-8 rounded-lg bg-primary px-3 text-xs font-semibold text-white disabled:opacity-50"
                      >
                        Link
                      </button>
                    </div>
                    {detail.events.length === 0 ? (
                      <div className="text-xs text-slate-500">No related events recorded</div>
                    ) : detail.events.map((event) => (
                      <details key={event.id} className="rounded-lg border border-slate-800 bg-slate-950/25 text-[11px] text-slate-400">
                        <summary className="cursor-pointer px-2 py-1.5 font-semibold text-slate-300">{event.event_type}</summary>
                        <pre className="max-h-32 overflow-auto border-t border-slate-800 p-2 font-mono text-[10px] text-slate-500">{formatEventPayload(event)}</pre>
                      </details>
                    ))}
                  </div>

                  <div className="space-y-2">
                    <div className="flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-wider text-slate-500">
                      <Activity className="h-3 w-3 text-primary/80" /> Worker Logs
                    </div>
                    {!detail.logs.stdout && !detail.logs.stderr ? (
                      <div className="text-xs text-slate-500">No worker logs recorded yet</div>
                    ) : (
                      <div className="space-y-2">
                        {detail.logs.stdout ? (
                          <details open className="rounded-lg border border-slate-800 bg-slate-950/25 text-[11px] text-slate-400">
                            <summary className="cursor-pointer px-2 py-1.5 font-semibold text-slate-300">
                              stdout · {detail.logs.stdout_path}
                            </summary>
                            <pre className="max-h-48 overflow-auto whitespace-pre-wrap border-t border-slate-800 p-2 font-mono text-[10px] leading-relaxed text-slate-400">
                              {detail.logs.stdout}
                            </pre>
                          </details>
                        ) : null}
                        {detail.logs.stderr ? (
                          <details open className="rounded-lg border border-slate-800 bg-slate-950/25 text-[11px] text-slate-400">
                            <summary className="cursor-pointer px-2 py-1.5 font-semibold text-slate-300">
                              stderr · {detail.logs.stderr_path}
                            </summary>
                            <pre className="max-h-48 overflow-auto whitespace-pre-wrap border-t border-slate-800 p-2 font-mono text-[10px] leading-relaxed text-amber-100/80">
                              {detail.logs.stderr}
                            </pre>
                          </details>
                        ) : null}
                      </div>
                    )}
                  </div>
                </div>
              ) : detailLoading ? (
                <div className="text-xs text-slate-500">Loading detail...</div>
              ) : (
                <div className="text-xs text-slate-500">Select another task or refresh the board.</div>
              )}
            </div>
          </aside>
        ) : null}
      </div>
    </div>
  );
}
