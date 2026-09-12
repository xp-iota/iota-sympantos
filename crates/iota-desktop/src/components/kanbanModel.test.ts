import test from "node:test";
import assert from "node:assert/strict";
import {
  buildKanbanColumns,
  filterKanbanTasks,
  formatDispatchReport,
  groupRunningByLane,
  legalStatusActions,
} from "./kanbanModel";
import type { KanbanTask } from "../types";

function task(id: number, status: KanbanTask["status"], overrides: Partial<KanbanTask> = {}): KanbanTask {
  return {
    id,
    board_id: 1,
    title: `Task ${id}`,
    body: "",
    status,
    priority: 0,
    tags: [],
    created_at: 1,
    updated_at: 1,
    claim_ttl_secs: 0,
    ...overrides,
  };
}

test("buildKanbanColumns returns all six visible board columns in order", () => {
  const columns = buildKanbanColumns([task(1, "ready"), task(2, "running"), task(3, "archived")]);
  assert.deepEqual(
    columns.map((column) => column.status),
    ["triage", "todo", "ready", "running", "blocked", "done"],
  );
  assert.deepEqual(
    columns.find((column) => column.status === "ready")?.tasks.map((item) => item.id),
    [1],
  );
  assert.equal(
    columns.some((column) => column.tasks.some((item) => item.status === "archived")),
    false,
  );
});

test("filterKanbanTasks matches search, board, and assignee", () => {
  const tasks = [
    task(1, "ready", { title: "Implement API", assignee: "backend-dev", tags: ["auth"] }),
    task(2, "ready", { title: "Review copy", assignee: "reviewer", tags: ["docs"] }),
  ];
  assert.deepEqual(
    filterKanbanTasks(tasks, { search: "auth", assignee: "backend-dev", boardId: 1 }).map((item) => item.id),
    [1],
  );
  assert.deepEqual(filterKanbanTasks(tasks, { search: "missing", assignee: "all", boardId: "all" }), []);
});

test("groupRunningByLane groups missing assignee under default", () => {
  const groups = groupRunningByLane([task(1, "running", { assignee: "backend-dev" }), task(2, "running")]);
  assert.deepEqual(
    groups.map((group) => group.lane),
    ["backend-dev", "default"],
  );
  assert.deepEqual(
    groups.map((group) => group.tasks.length),
    [1, 1],
  );
});

test("legalStatusActions only exposes valid transitions", () => {
  assert.deepEqual(
    legalStatusActions("blocked").map((action) => action.to),
    ["ready", "done"],
  );
  assert.deepEqual(
    legalStatusActions("done").map((action) => action.to),
    ["archived"],
  );
  assert.deepEqual(legalStatusActions("ready"), []);
});

test("formatDispatchReport makes a no-op nudge visible", () => {
  assert.equal(
    formatDispatchReport({
      spawned: 0,
      completed: 0,
      timed_out: 0,
      spawn_failures: 0,
      reclaimed: 0,
      active_workers: 1,
    }),
    "No ready tasks · 1 active worker",
  );
});
