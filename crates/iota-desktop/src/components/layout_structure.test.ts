import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { clampInspectorWidth, defaultInspectorWidth } from "./ChatWorkbench";

test("MemoryContextWorkspace is hosted by the right inspector, not the central workbench", () => {
  const workbench = readFileSync("src/components/ChatWorkbench.tsx", "utf8");
  const inspector = readFileSync("src/components/RightInspector.tsx", "utf8");

  assert.equal(workbench.includes("MemoryContextWorkspace"), false);
  assert.equal(workbench.includes('view === "memory"'), false);
  assert.equal(inspector.includes("MemoryContextWorkspace"), true);
  assert.equal(inspector.includes("Observability"), true);
  assert.equal(inspector.includes("Memory / Context"), false);
  assert.match(inspector, />\s*Memory\s*</);
  assert.match(inspector, />\s*Context\s*</);
  assert.equal(inspector.includes('mode="memory"'), true);
  assert.equal(inspector.includes('mode="context"'), true);
});

test("inspector splitter can expand past the old fixed desktop cap", () => {
  assert.equal(clampInspectorWidth(1100, 1440), 1100);
  assert.equal(clampInspectorWidth(1300, 1440), 1120);
  assert.equal(clampInspectorWidth(200, 1440), 360);
});

test("default inspector split starts at a 1:2 left/right ratio", () => {
  assert.equal(defaultInspectorWidth(1440), 960);
  assert.equal(1440 - defaultInspectorWidth(1440), 480);
});

test("left panel controls are hosted in the lower control bar", () => {
  const workbench = readFileSync("src/components/ChatWorkbench.tsx", "utf8");
  const headerStart = workbench.indexOf("<header");
  const headerEnd = workbench.indexOf("</header>", headerStart);
  const header = workbench.slice(headerStart, headerEnd);

  assert.equal(header.includes("workspaceControls"), false);
  assert.match(workbench, /mb-3 flex justify-start[\s\S]*\{workspaceControls\}/);
  assert.match(workbench, /border-t border-slate-800\/40[\s\S]*\{workspaceControls\}/);
});
