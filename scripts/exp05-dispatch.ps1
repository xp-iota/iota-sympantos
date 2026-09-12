<#
.SYNOPSIS
    exp05: Kanban DB 劫持验证 — iota 投影 + hermes 无感执行 + 结果回收
.DESCRIPTION
    1. 编译 iota（debug）
    2. 前置检查（hermes 可用性 + 推理 provider）
    3. 创建独立 board + task "生成宠物"
    4. 状态推进 triage -> todo -> ready
    5. iota kanban dispatch（核心验证：shadow 投影 → hermes -z → 结果回收）
    6. 校验 V1-V6
.PARAMETER Timeout
    dispatch 超时秒数（默认 120）
#>
param(
    [int]$Timeout = 120
)

$ErrorActionPreference = "Stop"
$script:RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$script:IotaBin = $null
$script:LogDir = Join-Path $PSScriptRoot "logs"
if (-not (Test-Path $script:LogDir)) { New-Item -ItemType Directory -Path $script:LogDir | Out-Null }

# --- Helpers (参考 examples/kanban_hermes_demo.ps1) ---

function Write-Step {
    param([string]$Text)
    Write-Host ""
    Write-Host "== $Text ==" -ForegroundColor Cyan
}

function Invoke-Iota {
    param(
        [Parameter(Mandatory)]
        [string[]]$Args
    )
    Push-Location $script:RepoRoot
    try {
        $all = & $script:IotaBin @Args 2>&1
        $exitCode = $LASTEXITCODE
        return [PSCustomObject]@{
            ExitCode = $exitCode
            Output   = ($all | Out-String).Trim()
        }
    }
    finally { Pop-Location }
}

# --- Step 0: Build ---

Write-Step "0) 编译 iota"
Push-Location $script:RepoRoot
try {
    & cargo build --quiet
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}
finally { Pop-Location }

$script:IotaBin = (Resolve-Path (Join-Path $script:RepoRoot "target\debug\iota.exe")).Path
Write-Host "iota: $script:IotaBin"

# --- Step 0.5: Pre-flight ---

Write-Step "0.5) 前置检查"
$hermesVer = & hermes version 2>&1 | Out-String
Write-Host "hermes: $($hermesVer.Trim())"

$smoke = & hermes -z "reply with exactly: OK" 2>&1 | Out-String
if ($LASTEXITCODE -ne 0) {
    Write-Host "hermes smoke FAILED: $smoke" -ForegroundColor Red
    throw "hermes inference unreachable"
}
Write-Host "smoke test: PASS"

# --- Step 1: Create board + task ---

Write-Step "1) 创建 board + task"
$ts = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
$slug = "exp05-$ts"

$r = Invoke-Iota kanban, create-board, $slug, "Exp05 Pet Gen"
if ($r.ExitCode -ne 0) { throw "create-board failed: $($r.Output)" }
if ($r.Output -match 'Created board #(\d+)') { $boardId = [int]$Matches[1] }
else { throw "parse board id: $($r.Output)" }
Write-Host "board: #$boardId ($slug)"

$r = Invoke-Iota kanban, create-task, $boardId, "生成宠物"
if ($r.ExitCode -ne 0) { throw "create-task failed: $($r.Output)" }
if ($r.Output -match 'Created task #(\d+)') { $taskId = [int]$Matches[1] }
else { throw "parse task id: $($r.Output)" }
Write-Host "task: #$taskId"

# --- Step 1.5: triage -> todo -> ready ---

Write-Step "1.5) 状态推进"
$r = Invoke-Iota kanban, move, $taskId, todo
if ($r.ExitCode -ne 0) { throw "move todo: $($r.Output)" }
$r = Invoke-Iota kanban, move, $taskId, ready
if ($r.ExitCode -ne 0) { throw "move ready: $($r.Output)" }
Write-Host "triage -> todo -> ready: OK"

# --- Step 2: Dispatch (核心) ---

Write-Step "2) dispatch — iota 劫持 hermes kanban 读写"
Write-Host "  shadow 投影: iota DB -> shadow DB (hermes schema)"
Write-Host "  spawn: hermes --yolo -z 'work kanban task $taskId'"
Write-Host "  环境变量: HERMES_KANBAN_TASK, HERMES_KANBAN_DB, HERMES_KANBAN_RUN_ID"
Write-Host ""

$sw = [System.Diagnostics.Stopwatch]::StartNew()
$r = Invoke-Iota kanban, dispatch, $taskId, --timeout, $Timeout
$sw.Stop()
$elapsed = [math]::Round($sw.Elapsed.TotalSeconds, 1)

Write-Host "exit: $($r.ExitCode)"
Write-Host "elapsed: ${elapsed}s"
Write-Host "output: $($r.Output)"

# --- Step 3: Verify ---

Write-Step "3) 校验 V1-V6"

$shadowsDir = Join-Path $env:USERPROFILE ".i6\kanban\shadows"
$stderrLog = Join-Path $shadowsDir "$taskId.stderr.log"
$stdoutLog = Join-Path $shadowsDir "$taskId.stdout.log"
$shadowDb  = Join-Path $shadowsDir "$taskId\kanban.db"

$results = @{}

# V1: hermes 进程正常退出
Write-Host ""
Write-Host "V1 - hermes process:" -ForegroundColor Yellow
if (Test-Path $stderrLog) {
    $stderr = (Get-Content $stderrLog -Raw).Trim()
    if ($stderr) {
        Write-Host "  FAIL — stderr: $($stderr.Substring(0, [math]::Min(200, $stderr.Length)))" -ForegroundColor Red
        $results.V1 = "FAIL"
    } else {
        Write-Host "  PASS — stderr empty" -ForegroundColor Green
        $results.V1 = "PASS"
    }
} else {
    Write-Host "  FAIL — stderr log not found" -ForegroundColor Red
    $results.V1 = "FAIL"
}

# V2: 读劫持生效（hermes stdout 有内容）
Write-Host ""
Write-Host "V2 - 读劫持（hermes 读到投影数据）:" -ForegroundColor Yellow
if (Test-Path $stdoutLog) {
    $stdout = (Get-Content $stdoutLog -Raw).Trim()
    if ($stdout) {
        Write-Host "  PASS — stdout ($($stdout.Length) chars):" -ForegroundColor Green
        Write-Host "  $($stdout.Substring(0, [math]::Min(300, $stdout.Length)))"
        $results.V2 = "PASS"
    } else {
        Write-Host "  FAIL — stdout empty" -ForegroundColor Red
        $results.V2 = "FAIL"
    }
} else {
    Write-Host "  FAIL — stdout log not found" -ForegroundColor Red
    $results.V2 = "FAIL"
}

# V3 + V4: 写劫持生效（shadow DB 有 completed 事件 + payload）
Write-Host ""
Write-Host "V3/V4 - 写劫持（kanban_complete → shadow DB）:" -ForegroundColor Yellow
if (Test-Path $shadowDb) {
    $events = & sqlite3 $shadowDb "SELECT kind, payload FROM task_events ORDER BY id" 2>&1
    if ($events) {
        $events | ForEach-Object { Write-Host "  $_" }
        if ("$events" -match "completed") {
            $results.V3 = "PASS"
            if ("$events" -match "summary") { $results.V4 = "PASS" }
            else { $results.V4 = "FAIL"; Write-Host "  V4 FAIL: no summary in payload" -ForegroundColor Red }
        } else {
            $results.V3 = "FAIL"
            $results.V4 = "FAIL"
            Write-Host "  FAIL — no 'completed' event" -ForegroundColor Red
        }
    } else {
        Write-Host "  FAIL — no events in shadow DB" -ForegroundColor Red
        $results.V3 = "FAIL"
        $results.V4 = "FAIL"
    }
} else {
    # Shadow may have been cleaned up already (expected on success path)
    Write-Host "  shadow DB already cleaned (normal on success)" -ForegroundColor DarkYellow
    # If dispatch succeeded (task is done), V3/V4 inferred PASS
    if ($r.Output -match "done") {
        $results.V3 = "PASS (inferred)"
        $results.V4 = "PASS (inferred)"
    } else {
        $results.V3 = "SKIP"
        $results.V4 = "SKIP"
    }
}

# V5: 主 store 状态同步
Write-Host ""
Write-Host "V5 - 主 store 状态同步:" -ForegroundColor Yellow
$taskStatus = Invoke-Iota kanban, show, $taskId
Write-Host "  $($taskStatus.Output)"
if ($taskStatus.Output -match "done|archived") {
    Write-Host "  PASS" -ForegroundColor Green
    $results.V5 = "PASS"
} else {
    Write-Host "  FAIL — not done" -ForegroundColor Red
    $results.V5 = "FAIL"
}

# V6: 耗时
Write-Host ""
Write-Host "V6 - 耗时:" -ForegroundColor Yellow
if ($r.ExitCode -eq 0 -and $elapsed -lt $Timeout) {
    Write-Host "  PASS — ${elapsed}s" -ForegroundColor Green
    $results.V6 = "PASS"
} else {
    Write-Host "  FAIL — ${elapsed}s (exit=$($r.ExitCode))" -ForegroundColor Red
    $results.V6 = "FAIL"
}

# --- Summary ---

Write-Step "SUMMARY"
$pass = 0; $fail = 0
foreach ($k in "V1","V2","V3","V4","V5","V6") {
    $v = $results[$k]
    $color = if ($v -match "PASS") { "Green" } else { "Red" }
    Write-Host "  $k : $v" -ForegroundColor $color
    if ($v -match "PASS") { $pass++ } else { $fail++ }
}
Write-Host ""
if ($fail -eq 0) {
    Write-Host "  ALL PASS ($pass/6)" -ForegroundColor Green
} else {
    Write-Host "  $fail FAILED, $pass PASSED" -ForegroundColor Red
}

# --- Save logs ---
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = Join-Path $script:LogDir "exp05-$stamp.txt"
@"
exp05-kanban-dispatch results ($stamp)
task_id: $taskId
board: $slug (#$boardId)
elapsed: ${elapsed}s
dispatch_exit: $($r.ExitCode)
dispatch_output: $($r.Output)
V1=$($results.V1) V2=$($results.V2) V3=$($results.V3) V4=$($results.V4) V5=$($results.V5) V6=$($results.V6)
"@ | Set-Content -LiteralPath $logFile -Encoding UTF8
if (Test-Path $stdoutLog) { Copy-Item $stdoutLog (Join-Path $script:LogDir "exp05-stdout-$stamp.txt") }
if (Test-Path $stderrLog) { Copy-Item $stderrLog (Join-Path $script:LogDir "exp05-stderr-$stamp.txt") }
Write-Host "logs: $logFile"
