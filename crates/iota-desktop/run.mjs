#!/usr/bin/env node
import { spawn } from "node:child_process";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const desktopDir = path.dirname(fileURLToPath(import.meta.url));
const script = path.resolve(desktopDir, "..", "..", "scripts", "dev-desktop.mjs");
const node = process.execPath;

const child = spawn(node, [script, ...process.argv.slice(2)], {
  cwd: desktopDir,
  env: process.env,
  stdio: "inherit",
  windowsHide: false,
});

child.on("error", (error) => {
  console.error(`failed to start desktop development script: ${error.message}`);
  process.exitCode = 1;
});

child.on("exit", (code, signal) => {
  process.exitCode = code ?? (signal ? 1 : 0);
});
