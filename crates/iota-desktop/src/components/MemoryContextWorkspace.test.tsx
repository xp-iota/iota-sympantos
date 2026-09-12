import { MemoryContextWorkspace } from "./MemoryContextWorkspace";
import assert from "node:assert/strict";
import test from "node:test";

test("MemoryContextWorkspace is a function component", () => {
  assert.equal(typeof MemoryContextWorkspace, "function");
});
