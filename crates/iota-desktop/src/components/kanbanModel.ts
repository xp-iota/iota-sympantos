import type { KanbanDispatchReport, KanbanStatus, KanbanTask } from "../types";

export const BOARD_STATUSES = ["triage", "todo", "ready", "running", "blocked", "done"] as const satisfies readonly KanbanStatus[];

export const STATUS_LABELS: Record<KanbanStatus, string> = {
  triage: "Triage",
  todo: "Todo",
  ready: "Ready",
  running: "Running",
  blocked: "Blocked",
  done: "Done",
  archived: "Archived",
};

export type KanbanFilters = {
  search: string;
  assignee: string | "all";
  boardId: number | "all";
};

export function formatDispatchReport(report?: KanbanDispatchReport | null) {
  if (!report) return "Auto-dispatch is active";
  const parts = [
    report.spawned ? `${report.spawned} spawned` : undefined,
    report.completed ? `${report.completed} completed` : undefined,
    report.reclaimed ? `${report.reclaimed} reclaimed` : undefined,
    report.timed_out ? `${report.timed_out} timed out` : undefined,
    report.spawn_failures ? `${report.spawn_failures} failed` : undefined,
  ].filter(Boolean);
  const workerLabel = report.active_workers === 1 ? "active worker" : "active workers";
  return parts.length > 0 ? parts.join(" · ") : `No ready tasks · ${report.active_workers} ${workerLabel}`;
}

export function filterKanbanTasks(tasks: KanbanTask[], filters: KanbanFilters) {
  const needle = filters.search.trim().toLowerCase();
  return tasks.filter((task) => {
    if (filters.boardId !== "all" && task.board_id !== filters.boardId) return false;
    if (filters.assignee !== "all" && (task.assignee ?? "default") !== filters.assignee) return false;
    if (!needle) return true;
    const haystack = [task.title, task.body ?? "", task.assignee ?? "", ...task.tags].join(" ").toLowerCase();
    return haystack.includes(needle);
  });
}

export function buildKanbanColumns(tasks: KanbanTask[]) {
  return BOARD_STATUSES.map((status) => ({
    status,
    label: STATUS_LABELS[status],
    tasks: tasks.filter((task) => task.status === status),
  }));
}

export function groupRunningByLane(tasks: KanbanTask[]) {
  const map = new Map<string, KanbanTask[]>();
  for (const task of tasks.filter((item) => item.status === "running")) {
    const lane = task.assignee || "default";
    map.set(lane, [...(map.get(lane) ?? []), task]);
  }
  return [...map.entries()]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([lane, laneTasks]) => ({ lane, tasks: laneTasks }));
}

export function uniqueAssignees(tasks: KanbanTask[]) {
  return [...new Set(tasks.map((task) => task.assignee || "default"))].sort((left, right) => left.localeCompare(right));
}

export function legalStatusActions(status: KanbanStatus): Array<{ label: string; to: KanbanStatus }> {
  switch (status) {
    case "triage":
      return [{ label: "Move to Todo", to: "todo" }];
    case "todo":
      return [{ label: "Move to Ready", to: "ready" }];
    case "running":
      return [
        { label: "Mark Done", to: "done" },
        { label: "Block", to: "blocked" },
        { label: "Requeue", to: "ready" },
      ];
    case "blocked":
      return [
        { label: "Unblock", to: "ready" },
        { label: "Mark Done", to: "done" },
      ];
    case "done":
      return [{ label: "Archive", to: "archived" }];
    default:
      return [];
  }
}
