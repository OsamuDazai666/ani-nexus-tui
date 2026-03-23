# ─────────────────────────────────────────────────────────────────────────────
# nexus-tui installer — Windows (PowerShell)
# Run with: powershell -ExecutionPolicy Bypass -File install.ps1
# ─────────────────────────────────────────────────────────────────────────────
$ErrorActionPreference = "Stop"
$ProgressPreference    = "SilentlyContinue"  # speeds up Invoke-WebRequest

$INSTALL_DIR = Join-Path $env:APPDATA "nexus-tui"
$BIN_DIR     = Join-Path $env:LOCALAPPDATA "Programs\nexus-tui"
$REPO_URL    = "https://github.com/OsamuDazai666/nexus-tui.git"
$EXE         = Join-Path $INSTALL_DIR "target\release\nexus.exe"
$BIN_EXE     = Join-Path $BIN_DIR "nexus.exe"

# ── Colours ───────────────────────────────────────────────────────────────────
function Write-Header {
    Clear-Host
    Write-Host ""
    Write-Host "  " -NoNewline
    Write-Host "◆ " -ForegroundColor Yellow -NoNewline
    Write-Host "NEXUS-TUI INSTALLER" -ForegroundColor White
    Write-Host "  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor DarkGray
    Write-Host ""
}

function Write-Step($msg) {
    Write-Host "  " -NoNewline
    Write-Host "▶ " -ForegroundColor Cyan -NoNewline
    Write-Host $msg -ForegroundColor White
}

function Write-OK($msg) {
    Write-Host "    " -NoNewline
    Write-Host "✓ " -ForegroundColor Green -NoNewline
    Write-Host $msg -ForegroundColor Gray
}

function Write-Fail($msg) {
    Write-Host "    " -NoNewline
    Write-Host "✗ " -ForegroundColor Red -NoNewline
    Write-Host $msg -ForegroundColor Red
    exit 1
}

function Write-Info($msg)  { Write-Host "    $msg" -ForegroundColor DarkGray }
function Write-Warn($msg)  {
    Write-Host "    " -NoNewline
    Write-Host "⚠  " -ForegroundColor Yellow -NoNewline
    Write-Host $msg -ForegroundColor Yellow
}

function Ask-YesNo($msg) {
    Write-Host "  " -NoNewline
    Write-Host "? " -ForegroundColor Cyan -NoNewline
    Write-Host "$msg " -ForegroundColor White -NoNewline
    Write-Host "[Y/n] " -ForegroundColor DarkGray -NoNewline
    $ans = Read-Host
    return ($ans -eq "" -or $ans -match "^[Yy]")
}

function Start-Spinner($msg) {
    $script:SpinMsg  = $msg
    $script:SpinStop = $false
    $frames = @('⠋','⠙','⠹','⠸','⠼','⠴','⠦','⠧','⠇','⠏')
    $script:SpinJob = Start-Job -ScriptBlock {
        param($frames, $msg, $pipe)
        $i = 0
        while (-not $using:SpinStop) {
            $f = $frames[$i % $frames.Count]
            Write-Host "`r    $f  $msg" -NoNewline
            Start-Sleep -Milliseconds 100
            $i++
        }
    } -ArgumentList $frames, $msg, $null
}

function Stop-Spinner {
    $script:SpinStop = $true
    Start-Sleep -Milliseconds 200
    if ($script:SpinJob) { Remove-Job $script:SpinJob -Force -ErrorAction SilentlyContinue }
    Write-Host "`r" -NoNewline
    Write-Host "                                        `r" -NoNewline
}

function Check-Command($cmd) {
    return $null -ne (Get-Command $cmd -ErrorAction SilentlyContinue)
}

function Get-Version($cmd) {
    try { return (& $cmd --version 2>&1) -replace ".*?(\d+\.\d+[\.\d]*).*",'$1' | Select-Object -First 1 }
    catch { return "?" }
}

# ── Main ──────────────────────────────────────────────────────────────────────
Write-Header

# ── Detect existing install ───────────────────────────────────────────────────
$skipClone = $false
if (Test-Path (Join-Path $INSTALL_DIR ".git")) {
    Write-Host "  " -NoNewline
    Write-Host "Existing install found at $INSTALL_DIR" -ForegroundColor Yellow
    Write-Host ""

    Write-Step "Checking for updates"
    Set-Location $INSTALL_DIR
    git fetch origin --quiet 2>$null | Out-Null
    $local  = git rev-parse HEAD 2>$null
    $remote = git rev-parse origin/main 2>$null

    if ($local -eq $remote) {
        Write-OK "Already up to date"
        Write-Host ""
        Write-Info "Binary: $BIN_EXE"
        Write-Host ""
        Write-Host "  ◆ Nothing to do. Run " -ForegroundColor Gray -NoNewline
        Write-Host "nexus" -ForegroundColor Cyan -NoNewline
        Write-Host " to launch." -ForegroundColor Gray
        Write-Host ""
        exit 0
    }

    $commits = (git log --oneline "${local}..${remote}" 2>$null | Measure-Object -Line).Lines
    Write-Info "$commits new commit(s) available"
    Write-Host ""

    if (Ask-YesNo "Update nexus-tui?") {
        Write-Step "Pulling latest"
        git pull origin main --quiet 2>$null | Out-Null
        Write-OK "Updated"
        $skipClone = $true
    } else {
        Write-Host ""
        Write-Host "  Skipped. Run " -ForegroundColor DarkGray -NoNewline
        Write-Host "nexus" -ForegroundColor Cyan -NoNewline
        Write-Host " to launch." -ForegroundColor DarkGray
        Write-Host ""; exit 0
    }
} else {
    Write-Info "Install directory: $INSTALL_DIR"
    Write-Host ""
    if (-not (Ask-YesNo "Install nexus-tui?")) {
        Write-Host ""; Write-Host "  Cancelled." -ForegroundColor DarkGray; Write-Host ""; exit 0
    }
}

Write-Host ""

# ── Check dependencies ────────────────────────────────────────────────────────
Write-Step "Checking dependencies"

$hasGit  = Check-Command "git"
$hasRust = Check-Command "rustc"
$hasMpv  = Check-Command "mpv"
$hasWinget = Check-Command "winget"

if ($hasGit)  { Write-OK "git $(Get-Version git)" }
else          { Write-Fail "git is required. Install from https://git-scm.com" }

if ($hasRust) { Write-OK "rust $(Get-Version rustc)" }
if ($hasMpv)  { Write-OK "mpv $(Get-Version mpv)" }
else          { Write-Warn "mpv not found — install it to play anime (winget install mpv)" }

# ── Install Rust if missing ───────────────────────────────────────────────────
if (-not $hasRust) {
    Write-Host ""
    Write-Step "Installing Rust"
    if (Ask-YesNo "Install Rust via rustup-init.exe?") {
        $rustupUrl = "https://win.rustup.rs/x86_64"
        $rustupExe = Join-Path $env:TEMP "rustup-init.exe"
        Write-Info "Downloading rustup…"
        Invoke-WebRequest -Uri $rustupUrl -OutFile $rustupExe
        Write-Info "Running installer (quiet mode)…"
        & $rustupExe -y --quiet
        # Reload PATH
        $env:PATH = [System.Environment]::GetEnvironmentVariable("PATH","Machine") + ";" +
                    [System.Environment]::GetEnvironmentVariable("PATH","User")
        if (Check-Command "rustc") { Write-OK "Rust installed" }
        else { Write-Fail "Rust install failed. Please install manually from https://rustup.rs" }
    } else {
        Write-Fail "Rust is required to build nexus-tui."
    }
}

Write-Host ""

# ── Clone repo ────────────────────────────────────────────────────────────────
if (-not $skipClone) {
    Write-Step "Cloning repository"
    if (Test-Path $INSTALL_DIR) { Remove-Item $INSTALL_DIR -Recurse -Force }
    New-Item -ItemType Directory -Path (Split-Path $INSTALL_DIR) -Force | Out-Null
    $job = Start-Job { git clone --quiet $using:REPO_URL $using:INSTALL_DIR }
    $i = 0; $frames = @('⠋','⠙','⠹','⠸','⠼','⠴','⠦','⠧','⠇','⠏')
    while ($job.State -eq "Running") {
        Write-Host "`r    $($frames[$i % 10])  Cloning nexus-tui…" -NoNewline
        Start-Sleep -Milliseconds 100; $i++
    }
    Write-Host "`r                                      `r" -NoNewline
    Receive-Job $job | Out-Null; Remove-Job $job
    Write-OK "Cloned to $INSTALL_DIR"
    Write-Host ""
}

# ── Build ─────────────────────────────────────────────────────────────────────
Write-Step "Building nexus-tui"
Write-Info "This takes 1–3 minutes on first build"
Write-Host ""

Set-Location $INSTALL_DIR
$start = Get-Date

$env:CARGO_INCREMENTAL = "0"
$job = Start-Job { Set-Location $using:INSTALL_DIR; cargo build --release --quiet 2>&1 }

$i = 0; $frames = @('⠋','⠙','⠹','⠸','⠼','⠴','⠦','⠧','⠇','⠏')
while ($job.State -eq "Running") {
    Write-Host "`r    $($frames[$i % 10])  Compiling…" -NoNewline
    Start-Sleep -Milliseconds 100; $i++
}
Write-Host "`r                              `r" -NoNewline

$output = Receive-Job $job
Remove-Job $job

if (-not (Test-Path $EXE)) {
    Write-Fail "Build failed.`n$output"
}

$elapsed = [int]((Get-Date) - $start).TotalSeconds
Write-OK "Built in ${elapsed}s"
Write-Host ""

# ── Install binary ────────────────────────────────────────────────────────────
Write-Step "Installing binary"
New-Item -ItemType Directory -Path $BIN_DIR -Force | Out-Null
Copy-Item $EXE $BIN_EXE -Force
Write-OK "Installed to $BIN_EXE"

# ── PATH check ────────────────────────────────────────────────────────────────
$userPath = [System.Environment]::GetEnvironmentVariable("PATH", "User")
if ($userPath -notlike "*$BIN_DIR*") {
    Write-Host ""
    Write-Warn "$BIN_DIR is not in your PATH"
    if (Ask-YesNo "Add it to your user PATH automatically?") {
        [System.Environment]::SetEnvironmentVariable("PATH", "$userPath;$BIN_DIR", "User")
        $env:PATH += ";$BIN_DIR"
        Write-OK "PATH updated (restart your terminal for it to take effect)"
    } else {
        Write-Info "Add this to your PATH manually: $BIN_DIR"
    }
}

# ── Done ──────────────────────────────────────────────────────────────────────
Write-Host ""
Write-Host "  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor DarkGray
Write-Host "  " -NoNewline
Write-Host "◆ " -ForegroundColor Yellow -NoNewline
Write-Host "Done!  Run " -ForegroundColor White -NoNewline
Write-Host "nexus" -ForegroundColor Cyan -NoNewline
Write-Host " to launch" -ForegroundColor White
Write-Host ""