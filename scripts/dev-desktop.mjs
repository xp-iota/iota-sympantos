#!/usr/bin/env node
import { execFileSync, spawn } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const ROOT_DIR = path.resolve(SCRIPT_DIR, "..");
const DESKTOP_DIR = path.join(ROOT_DIR, "crates", "iota-desktop");
const GRACE_MS = Number(process.env.IOTA_DEV_DAEMON_STOP_GRACE_SECONDS ?? "3") * 1000;
const DESKTOP_DEV_PORTS = (process.env.IOTA_DESKTOP_DEV_PORTS ?? "1420 1421")
  .split(/\s+/)
  .map((value) => Number(value))
  .filter((value) => Number.isInteger(value) && value > 0);

function usage() {
  console.log(`Usage: scripts/dev-desktop.mjs [--stop-only] [--] [extra npm tauri args...]

Stops existing iota daemon and desktop dev-server processes before starting the
Tauri desktop dev app.
The script builds the workspace iota CLI, installs frontend dependencies if
they are missing, and exports IOTA_CLI_PATH so Tauri autostarts the matching
daemon version.

Options:
  --stop-only   Stop daemon and desktop dev-server processes and exit.
  -h, --help    Show this help.

Environment:
  IOTA_DEV_DAEMON_STOP_GRACE_SECONDS  Seconds to wait before force-kill fallback. Default: 3.
  IOTA_DESKTOP_DEV_PORTS              Desktop dev ports to clear. Default: "1420 1421".
  CARGO_TARGET_DIR                    Optional Cargo target directory.`);
}

function commandOutput(command, args) {
  try {
    return execFileSync(command, args, { encoding: "utf8", windowsHide: true });
  } catch {
    return "";
  }
}

function listProcesses() {
  if (process.platform === "win32") {
    const output = commandOutput("powershell.exe", [
      "-NoProfile",
      "-Command",
      "Get-CimInstance Win32_Process | ForEach-Object { \"$($_.ProcessId)`t$($_.CommandLine)\" }",
    ]);
    return output
      .split(/\r?\n/)
      .map((line) => {
        const [pid, ...commandParts] = line.split("\t");
        return { pid: Number(pid), command: commandParts.join("\t") };
      })
      .filter((entry) => Number.isInteger(entry.pid) && entry.command);
  }

  const output = commandOutput("ps", ["-axo", "pid=,command="]);
  return output
    .split(/\r?\n/)
    .map((line) => {
      const match = line.trim().match(/^(\d+)\s+(.+)$/);
      return match ? { pid: Number(match[1]), command: match[2] } : null;
    })
    .filter(Boolean);
}

function findDaemonPids() {
  return [
    ...new Set(
      listProcesses()
        .filter(({ pid, command }) => {
          if (pid === process.pid || !/(^|\s)__daemon(\s|$)/.test(command)) {
            return false;
          }
          const normalized = command.replaceAll("\\", "/");
          const isIotaBinary = /(^|[\s/])iota(\.exe)?(\s|$)/i.test(normalized);
          const isCargoIota = /cargo(\.exe)?\s+run/i.test(command) && /--\s+__daemon/.test(command);
          return isIotaBinary || isCargoIota;
        })
        .map(({ pid }) => pid),
    ),
  ];
}

function findPortPids(ports) {
  if (ports.length === 0) {
    return [];
  }

  if (process.platform === "win32") {
    const portList = ports.join(",");
    const output = commandOutput("powershell.exe", [
      "-NoProfile",
      "-Command",
      `$ports = @(${portList}); Get-NetTCPConnection -State Listen -LocalPort $ports -ErrorAction SilentlyContinue | Select-Object -ExpandProperty OwningProcess`,
    ]);
    return uniquePids(output);
  }

  const lsofOutput = ports
    .map((port) => commandOutput("lsof", ["-nP", `-iTCP:${port}`, "-sTCP:LISTEN", "-Fp"]))
    .join("\n");
  return uniquePids(lsofOutput.replaceAll("p", ""));
}

function uniquePids(output) {
  return [
    ...new Set(
      output
        .split(/\s+/)
        .map((value) => Number(value.trim()))
        .filter((value) => Number.isInteger(value) && value > 0 && value !== process.pid),
    ),
  ];
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function killPid(pid, signal) {
  try {
    process.kill(pid, signal);
  } catch {
    // Process may already have exited.
  }
}

async function stopProcessGroup(label, initialPids, refind) {
  if (initialPids.length === 0) {
    console.log(`no ${label} processes found`);
    return;
  }

  console.log(`stopping ${label} process(es): ${initialPids.join(" ")}`);
  for (const pid of initialPids) {
    killPid(pid, process.platform === "win32" ? undefined : "SIGTERM");
  }

  const deadline = Date.now() + GRACE_MS;
  while (Date.now() < deadline) {
    const remaining = refind();
    if (remaining.length === 0) {
      console.log(`${label} stopped`);
      return;
    }
    await sleep(200);
  }

  const remaining = refind();
  if (remaining.length > 0) {
    console.log(`${label} still running after ${GRACE_MS / 1000}s; force-killing: ${remaining.join(" ")}`);
    for (const pid of remaining) {
      if (process.platform === "win32") {
        commandOutput("taskkill.exe", ["/PID", String(pid), "/T", "/F"]);
      } else {
        killPid(pid, "SIGKILL");
      }
    }
  }
}

function iotaCliPath() {
  const targetDir = process.env.CARGO_TARGET_DIR
    ? path.resolve(process.env.CARGO_TARGET_DIR)
    : path.join(ROOT_DIR, "target");
  const exeName = process.platform === "win32" ? "iota.exe" : "iota";
  return path.join(targetDir, "debug", exeName);
}

function viteBinPath() {
  const binName = process.platform === "win32" ? "vite.cmd" : "vite";
  return path.join(DESKTOP_DIR, "node_modules", ".bin", binName);
}

function runChecked(command, args, options = {}) {
  const child = spawn(commandName(command), args, {
    cwd: options.cwd ?? ROOT_DIR,
    env: options.env ?? process.env,
    shell: process.platform === "win32",
    stdio: "inherit",
    windowsHide: false,
  });
  return new Promise((resolve, reject) => {
    child.on("error", reject);
    child.on("exit", (code, signal) => {
      if (code === 0) {
        resolve();
      } else {
        reject(new Error(`${command} exited with ${signal ?? code}`));
      }
    });
  });
}

function commandName(command) {
  if (process.platform === "win32" && command === "npm") {
    return "npm.cmd";
  }
  return command;
}

async function buildIotaCli() {
  console.log("building current workspace iota CLI...");
  await runChecked("cargo", ["build", "-p", "iota-cli", "--bin", "iota"], { cwd: ROOT_DIR });

  const cliPath = iotaCliPath();
  if (!existsSync(cliPath)) {
    throw new Error(`built iota CLI was not found: ${cliPath}`);
  }
  process.env.IOTA_CLI_PATH = cliPath;
  console.log(`using IOTA_CLI_PATH=${cliPath}`);
}

async function ensureFrontendDeps() {
  // A one-click dev entry point cannot assume `npm install` was already
  // run. The local `vite` binary is the concrete artifact `tauri dev`'s
  // beforeDevCommand needs; its absence is what produces the confusing
  // "vite: command not found" failure, so treat it as the install signal
  // rather than only checking for the `node_modules` directory itself.
  if (existsSync(viteBinPath())) {
    return;
  }

  console.log(`frontend dependencies not found, running npm install in ${DESKTOP_DIR}...`);
  await runChecked("npm", ["install"], { cwd: DESKTOP_DIR });

  if (!existsSync(viteBinPath())) {
    throw new Error(
      `npm install completed but vite is still missing from ${path.join(DESKTOP_DIR, "node_modules", ".bin")}`,
    );
  }
}

async function main() {
  let stopOnly = process.env.npm_config_stop_only === "true";
  const extraArgs = [];
  const args = process.argv.slice(2);
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === "--stop-only") {
      stopOnly = true;
    } else if (arg === "-h" || arg === "--help") {
      usage();
      return;
    } else if (arg === "--") {
      extraArgs.push(...args.slice(index + 1));
      break;
    } else {
      extraArgs.push(arg);
    }
  }

  await stopProcessGroup("iota daemon", findDaemonPids(), findDaemonPids);
  await stopProcessGroup("desktop dev server", findPortPids(DESKTOP_DEV_PORTS), () =>
    findPortPids(DESKTOP_DEV_PORTS),
  );

  if (stopOnly) {
    return;
  }

  await buildIotaCli();
  await ensureFrontendDeps();
  await runChecked("npm", ["run", "tauri", "--", "dev", ...extraArgs], {
    cwd: DESKTOP_DIR,
    env: process.env,
  });
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
});
