<#
.SYNOPSIS
    Serene Lavoisier // Multi-Venue Trading System Orchestrator
    Manages background execution for the 3 core components:
      1. Rust Core Engine Daemon (TCP 127.0.0.1:9099)
      2. Python FastAPI Backend (HTTP 127.0.0.1:8000)
      3. Next.js Frontend Dashboard (HTTP 127.0.0.1:3000)

.EXAMPLE
    .\run.ps1            # Restarts all 3 components smoothly
    .\run.ps1 restart    # Restarts all 3 components
    .\run.ps1 start      # Starts components if not already running
    .\run.ps1 stop       # Cleanly terminates all 3 components
    .\run.ps1 status     # Checks health, ports, and PIDs
    .\run.ps1 logs       # Tails the component logs
#>

[CmdletBinding()]
param(
    [ValidateSet("start", "stop", "restart", "status", "logs")]
    [string]$Action = "restart",

    [ValidateSet("all", "engine", "backend", "frontend")]
    [string]$Component = "all",

    [switch]$Build,
    [string]$Tail = "backend"
)

$ErrorActionPreference = "Continue"
$ProjectRoot = $PSScriptRoot
$LogsDir = Join-Path $ProjectRoot "logs"
$PidFile = Join-Path $ProjectRoot ".pids.json"

if (-not (Test-Path $LogsDir)) {
    New-Item -ItemType Directory -Path $LogsDir -Force | Out-Null
}

function Get-PortProcess {
    param([int]$Port)
    $conns = Get-NetTCPConnection -LocalPort $Port -ErrorAction SilentlyContinue
    if ($conns) {
        $pids = $conns | Where-Object { $_.OwningProcess -gt 0 } | Select-Object -ExpandProperty OwningProcess -Unique
        return $pids
    }
    return @()
}

function Stop-ProcessTree {
    param([int]$ProcessId, [string]$Name = "Process")
    if ($ProcessId -gt 0) {
        try {
            $p = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
            if ($p) {
                Write-Host "  Stopping $Name (PID $ProcessId)..." -ForegroundColor DarkGray
                taskkill /PID $ProcessId /T /F 2>$null | Out-Null
            }
        } catch {}
    }
}

function Stop-Components {
    param([string]$Target = "all")
    Write-Host "`nStopping Serene Lavoisier services..." -ForegroundColor Cyan

    # Load tracked PIDs if present
    $trackedPids = @{}
    if (Test-Path $PidFile) {
        try {
            $trackedPids = Get-Content $PidFile -Raw | ConvertFrom-Json
        } catch {}
    }

    # 1. Engine (Port 9099)
    if ($Target -in @("all", "engine")) {
        $enginePortPids = Get-PortProcess -Port 9099
        foreach ($pidToKill in $enginePortPids) {
            Stop-ProcessTree -ProcessId $pidToKill -Name "Engine (Port 9099)"
        }
        if ($trackedPids.engine_pid) {
            Stop-ProcessTree -ProcessId $trackedPids.engine_pid -Name "Engine (Tracked PID)"
        }
    }

    # 2. Backend (Port 8000)
    if ($Target -in @("all", "backend")) {
        $backendPortPids = Get-PortProcess -Port 8000
        foreach ($pidToKill in $backendPortPids) {
            Stop-ProcessTree -ProcessId $pidToKill -Name "FastAPI Backend (Port 8000)"
        }
        if ($trackedPids.backend_pid) {
            Stop-ProcessTree -ProcessId $trackedPids.backend_pid -Name "FastAPI Backend (Tracked PID)"
        }
    }

    # 3. Frontend (Port 3000)
    if ($Target -in @("all", "frontend")) {
        $frontendPortPids = Get-PortProcess -Port 3000
        foreach ($pidToKill in $frontendPortPids) {
            Stop-ProcessTree -ProcessId $pidToKill -Name "Next.js Frontend (Port 3000)"
        }
        if ($trackedPids.frontend_pid) {
            Stop-ProcessTree -ProcessId $trackedPids.frontend_pid -Name "Next.js Frontend (Tracked PID)"
        }
    }

    Start-Sleep -Milliseconds 800
    if ($Target -eq "all") {
        Remove-Item $PidFile -Force -ErrorAction SilentlyContinue
    }
    Write-Host "Services stopped.`n" -ForegroundColor DarkGray
}

function Start-Components {
    param([string]$Target = "all", [switch]$DoBuild)
    Write-Host "`nStarting Serene Lavoisier services..." -ForegroundColor Cyan

    # Synchronize root .env to frontend/.env.local so Next.js always has latest configuration
    $rootEnv = Join-Path $ProjectRoot ".env"
    $frontendEnv = Join-Path $ProjectRoot "frontend\.env.local"
    if (Test-Path $rootEnv) {
        Copy-Item -Path $rootEnv -Destination $frontendEnv -Force
        Write-Host "  Synchronized root .env -> frontend/.env.local" -ForegroundColor DarkGray
    }

    $pidsToSave = @{}
    if (Test-Path $PidFile) {
        try {
            $existing = Get-Content $PidFile -Raw | ConvertFrom-Json
            if ($existing) {
                foreach ($prop in $existing.PSObject.Properties) {
                    $pidsToSave[$prop.Name] = $prop.Value
                }
            }
        } catch {}
    }

    # 1. Rust Engine Daemon
    if ($Target -in @("all", "engine")) {
        $engineExe = Join-Path $ProjectRoot "target\debug\engine-daemon.exe"
        Write-Host "Compiling Rust engine-daemon..." -ForegroundColor Yellow
        $cargoCmd = (Get-Command cargo -ErrorAction SilentlyContinue).Source
        if (-not $cargoCmd) {
            $cargoCmd = Join-Path $HOME ".cargo\bin\cargo.exe"
        }
        & $cargoCmd build -p engine-daemon
        if ($LASTEXITCODE -ne 0) {
            Write-Host "Build failed for engine-daemon!" -ForegroundColor Red
            return
        }

        $engineLog = Join-Path $LogsDir "engine.log"
        try { Clear-Content $engineLog -ErrorAction SilentlyContinue } catch {}
        $engineProc = Start-Process -FilePath $engineExe `
            -WorkingDirectory $ProjectRoot `
            -RedirectStandardOutput $engineLog `
            -RedirectStandardError (Join-Path $LogsDir "engine_error.log") `
            -PassThru -NoNewWindow

        $pidsToSave["engine_pid"] = $engineProc.Id
        Write-Host "  Started Engine Daemon      [PID $($engineProc.Id)] -> logs/engine.log" -ForegroundColor Green
    }

    # 2. Python FastAPI Backend
    if ($Target -in @("all", "backend")) {
        $backendLog = Join-Path $LogsDir "backend.log"
        try { Clear-Content $backendLog -ErrorAction SilentlyContinue } catch {}
        $pythonExe = (Get-Command python -ErrorAction SilentlyContinue).Source
        if (-not $pythonExe) {
            $pythonExe = "python"
        }

        $backendProc = Start-Process -FilePath $pythonExe `
            -ArgumentList "backend/run.py" `
            -WorkingDirectory $ProjectRoot `
            -RedirectStandardOutput $backendLog `
            -RedirectStandardError (Join-Path $LogsDir "backend_error.log") `
            -PassThru -NoNewWindow

        $pidsToSave["backend_pid"] = $backendProc.Id
        Write-Host "  Started FastAPI Backend    [PID $($backendProc.Id)] -> logs/backend.log" -ForegroundColor Green
    }

    # 3. Next.js Frontend
    if ($Target -in @("all", "frontend")) {
        $frontendLog = Join-Path $LogsDir "frontend.log"
        try { Clear-Content $frontendLog -ErrorAction SilentlyContinue } catch {}
        $frontendDir = Join-Path $ProjectRoot "frontend"

        $frontendProc = Start-Process -FilePath "cmd.exe" `
            -ArgumentList "/c npm run dev" `
            -WorkingDirectory $frontendDir `
            -RedirectStandardOutput $frontendLog `
            -RedirectStandardError (Join-Path $LogsDir "frontend_error.log") `
            -PassThru -NoNewWindow

        $pidsToSave["frontend_pid"] = $frontendProc.Id
        Write-Host "  Started Next.js Dashboard  [PID $($frontendProc.Id)] -> logs/frontend.log" -ForegroundColor Green
    }

    $pidsToSave | ConvertTo-Json | Set-Content $PidFile -Force

    # Wait for ports to become active
    Write-Host "`nWaiting for service listeners to become ready..." -ForegroundColor DarkGray
    $portsToCheck = @()
    if ($Target -in @("all", "engine")) { $portsToCheck += 9099 }
    if ($Target -in @("all", "backend")) { $portsToCheck += 8000 }
    if ($Target -in @("all", "frontend")) { $portsToCheck += 3000 }

    $allReady = $false
    for ($i = 0; $i -lt 15; $i++) {
        Start-Sleep -Milliseconds 600
        $activePorts = 0
        foreach ($p in $portsToCheck) {
            if ((Get-PortProcess -Port $p).Count -gt 0) {
                $activePorts++
            }
        }
        if ($activePorts -eq $portsToCheck.Count) {
            $allReady = $true
            break
        }
    }

    Show-Status
}

function Show-Status {
    Write-Host "`n=== Serene Lavoisier // Service Status ===" -ForegroundColor Cyan

    $services = @(
        @{ Name = "Rust Engine Daemon"; Port = 9099; Protocol = "TCP"; Key = "engine_pid" },
        @{ Name = "FastAPI Backend";   Port = 8000; Protocol = "HTTP"; Key = "backend_pid" },
        @{ Name = "Next.js Frontend";  Port = 3000; Protocol = "HTTP"; Key = "frontend_pid" }
    )

    $trackedPids = @{}
    if (Test-Path $PidFile) {
        try {
            $trackedPids = Get-Content $PidFile -Raw | ConvertFrom-Json
        } catch {}
    }

    foreach ($s in $services) {
        $pids = Get-PortProcess -Port $s.Port
        $isListening = ($pids.Count -gt 0)
        $pidText = if ($pids.Count -gt 0) { "PID: $($pids -join ', ')" } else { "PID: None" }

        if ($isListening) {
            Write-Host ("  {0,-20} [{1} {2,-4}] : " -f $s.Name, $s.Protocol, $s.Port) -NoNewline
            Write-Host "RUNNING" -ForegroundColor Green -NoNewline
            Write-Host " ($pidText)" -ForegroundColor DarkGray
        } else {
            Write-Host ("  {0,-20} [{1} {2,-4}] : " -f $s.Name, $s.Protocol, $s.Port) -NoNewline
            Write-Host "OFFLINE" -ForegroundColor Red
        }
    }

    $configuredPin = "123456"
    $rootEnv = Join-Path $ProjectRoot ".env"
    if (Test-Path $rootEnv) {
        $match = Select-String -Path $rootEnv -Pattern "^DASHBOARD_PIN\s*=\s*(.+)$"
        if ($match) { $configuredPin = $match.Matches[0].Groups[1].Value.Trim() }
    }

    Write-Host "`nEndpoints:" -ForegroundColor DarkGray
    Write-Host "  Dashboard UI    : http://localhost:3000  (Active PIN: $configuredPin)" -ForegroundColor White
    Write-Host "  Backend API     : http://localhost:8000/docs" -ForegroundColor White
    Write-Host "  Engine IPC      : 127.0.0.1:9099" -ForegroundColor White
    Write-Host "  Logs Folder     : ./logs/ (engine.log, backend.log, frontend.log)" -ForegroundColor White
    Write-Host "==========================================`n" -ForegroundColor Cyan
}

function Show-Logs {
    param([string]$LogName = "backend")
    $logFile = Join-Path $LogsDir "$LogName.log"
    if (Test-Path $logFile) {
        Write-Host "Tailing $LogName.log (Ctrl+C to exit)..." -ForegroundColor Cyan
        Get-Content $logFile -Wait -Tail 30
    } else {
        Write-Host "Log file not found: $logFile" -ForegroundColor Red
    }
}

# Main Execution Switch
switch ($Action) {
    "stop" {
        Stop-Components -Target $Component
    }
    "start" {
        Start-Components -Target $Component -DoBuild:$Build
    }
    "restart" {
        Stop-Components -Target $Component
        Start-Components -Target $Component -DoBuild:$Build
    }
    "status" {
        Show-Status
    }
    "logs" {
        Show-Logs -LogName $Tail
    }
}
