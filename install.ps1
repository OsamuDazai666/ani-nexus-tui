# nexus-tui installer for Windows
# Usage (run in PowerShell as Administrator):
#   irm https://raw.githubusercontent.com/OsamuDazai666/nexus-tui/main/install.ps1 | iex

$ErrorActionPreference = "Stop"
$REPO = "OsamuDazai666/nexus-tui"
$INSTALL_DIR = "$env:LOCALAPPDATA\nexus-tui"

function Write-Step  { Write-Host "  $args" -ForegroundColor Cyan }
function Write-Ok    { Write-Host "  ✓ $args" -ForegroundColor Green }
function Write-Warn  { Write-Host "  ⚠ $args" -ForegroundColor Yellow }

# ── Refresh PATH from environment (replaces refreshenv) ──────────────────────

function Update-SessionPath {
    $env:PATH = [System.Environment]::GetEnvironmentVariable("PATH", "Machine") + ";" +
                [System.Environment]::GetEnvironmentVariable("PATH", "User")
}

# ── Build from source ─────────────────────────────────────────────────────────

function Build-FromSource {
    # ── Git ───────────────────────────────────────────────────────────────────
    if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
        if (-not $HAS_WINGET) {
            Write-Warn "winget not found — please install Git manually from https://git-scm.com then re-run this script"
            exit 1
        }
        Write-Step "Installing Git..."
        winget install --id Git.Git -e --silent
        Update-SessionPath
    }

    # ── Rust (GNU toolchain — no Visual Studio needed) ────────────────────────
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        if (-not $HAS_WINGET) {
            Write-Warn "winget not found — please install Rust manually from https://rustup.rs then re-run this script"
            exit 1
        }
        Write-Step "Installing Rust..."
        winget install --id Rustlang.Rustup -e --silent
        Update-SessionPath
    }

    # Switch to GNU toolchain to avoid needing Visual Studio / MSVC linker
    Write-Step "Configuring Rust GNU toolchain (avoids ~3GB Visual Studio install)..."
    rustup toolchain install stable-x86_64-pc-windows-gnu --no-self-update | Out-Null
    rustup default stable-x86_64-pc-windows-gnu | Out-Null

    # ── mingw64 (provides the GNU linker) ─────────────────────────────────────
    if (-not (Get-Command x86_64-w64-mingw32-gcc -ErrorAction SilentlyContinue)) {
        Write-Step "Installing mingw-w64 (GNU linker for Rust)..."
        if ($HAS_SCOOP) {
            scoop install mingw
        } elseif ($HAS_WINGET) {
            winget install --id MSYS2.MSYS2 -e --silent
            # Add mingw64 bin to PATH
            $mingwBin = "C:\msys64\mingw64\bin"
            if (Test-Path $mingwBin) {
                $env:PATH += ";$mingwBin"
                $current = [System.Environment]::GetEnvironmentVariable("PATH", "User")
                [System.Environment]::SetEnvironmentVariable("PATH", "$current;$mingwBin", "User")
            }
        } else {
            Write-Warn "Could not install mingw-w64 automatically."
            Write-Host "    Please install Scoop first: https://scoop.sh" -ForegroundColor DarkGray
            exit 1
        }
        Update-SessionPath
    }

    # ── Clone & build ─────────────────────────────────────────────────────────
    $TMP = "$env:TEMP\nexus-build"
    if (Test-Path $TMP) { Remove-Item $TMP -Recurse -Force }

    Write-Step "Cloning nexus-tui..."
    git clone "https://github.com/$REPO.git" $TMP

    Push-Location $TMP
    Write-Step "Compiling... (this takes about a minute)"
    cargo build --release

    if (-not (Test-Path "target\release\nexus.exe")) {
        Pop-Location
        Write-Warn "Build failed — nexus.exe was not produced."
        exit 1
    }

    New-Item -ItemType Directory -Force -Path $INSTALL_DIR | Out-Null
    Copy-Item "target\release\nexus.exe" "$INSTALL_DIR\nexus.exe"
    Pop-Location
    Remove-Item $TMP -Recurse -Force
    Write-Ok "Built and installed to $INSTALL_DIR\nexus.exe"
}

Write-Host ""
Write-Host "  ◆ nexus-tui installer" -ForegroundColor Yellow -BackgroundColor Black
Write-Host ""

# ── Winget / Scoop check ──────────────────────────────────────────────────────

$HAS_WINGET = [bool](Get-Command winget -ErrorAction SilentlyContinue)
$HAS_SCOOP  = [bool](Get-Command scoop  -ErrorAction SilentlyContinue)

if (-not $HAS_SCOOP) {
    Write-Step "Installing Scoop package manager..."
    Set-ExecutionPolicy RemoteSigned -Scope CurrentUser -Force
    Invoke-RestMethod get.scoop.sh | Invoke-Expression
    $env:PATH += ";$env:USERPROFILE\scoop\shims"
    $HAS_SCOOP = $true
    Write-Ok "Scoop installed"
}

# ── Kitty ─────────────────────────────────────────────────────────────────────

if (-not (Get-Command kitty -ErrorAction SilentlyContinue)) {
    Write-Step "Installing Kitty terminal..."
    if ($HAS_WINGET) {
        winget install --id kovidgoyal.kitty -e --silent
    } else {
        scoop bucket add extras
        scoop install kitty
    }
    Write-Ok "Kitty installed"
} else {
    Write-Ok "Kitty already installed"
}

# ── mpv ───────────────────────────────────────────────────────────────────────

if (-not (Get-Command mpv -ErrorAction SilentlyContinue)) {
    Write-Step "Installing mpv..."
    if ($HAS_WINGET) {
        winget install --id mpv.mpv -e --silent
    } else {
        scoop install mpv
    }
    Write-Ok "mpv installed"
} else {
    Write-Ok "mpv already installed"
}

# ── yt-dlp ────────────────────────────────────────────────────────────────────

if (-not (Get-Command yt-dlp -ErrorAction SilentlyContinue)) {
    Write-Step "Installing yt-dlp..."
    scoop install yt-dlp
    Write-Ok "yt-dlp installed"
} else {
    Write-Ok "yt-dlp already installed"
}

# ── TMDB API Key ──────────────────────────────────────────────────────────────

$CURRENT_KEY = [System.Environment]::GetEnvironmentVariable("TMDB_API_KEY", "User")
if (-not $CURRENT_KEY) {
    Write-Host ""
    Write-Warn "TMDB API key not set (needed for Movies & TV)"
    Write-Host "    Get a free key: https://www.themoviedb.org/settings/api" -ForegroundColor DarkGray
    $KEY = Read-Host "    Enter TMDB API key (Enter to skip)"
    if ($KEY) {
        [System.Environment]::SetEnvironmentVariable("TMDB_API_KEY", $KEY, "User")
        $env:TMDB_API_KEY = $KEY
        Write-Ok "TMDB key saved to user environment"
    }
}

# ── Build & install nexus ─────────────────────────────────────────────────────

Write-Host ""
Write-Step "Building nexus-tui from source..."
Build-FromSource

# ── Add to PATH ───────────────────────────────────────────────────────────────

$CURRENT_PATH = [System.Environment]::GetEnvironmentVariable("PATH", "User")
if ($CURRENT_PATH -notlike "*$INSTALL_DIR*") {
    [System.Environment]::SetEnvironmentVariable(
        "PATH", "$CURRENT_PATH;$INSTALL_DIR", "User"
    )
    $env:PATH += ";$INSTALL_DIR"
    Write-Ok "Added $INSTALL_DIR to PATH"
}

# ── Desktop shortcut (opens in Kitty) ────────────────────────────────────────

$kittyCmd   = Get-Command kitty -ErrorAction SilentlyContinue
$KITTY_PATH = if ($kittyCmd) { $kittyCmd.Source } else { $null }

if ($KITTY_PATH) {
    $SHORTCUT_PATH = "$env:USERPROFILE\Desktop\nexus-tui.lnk"
    $WSH = New-Object -ComObject WScript.Shell
    $SC  = $WSH.CreateShortcut($SHORTCUT_PATH)
    $SC.TargetPath       = $KITTY_PATH
    $SC.Arguments        = "--title nexus-tui -- nexus"
    $SC.WorkingDirectory = $env:USERPROFILE
    $SC.Description      = "nexus-tui — Anime/Movies/TV/Manga browser"
    $SC.Save()
    Write-Ok "Desktop shortcut created (opens in Kitty)"
}

Write-Host ""
Write-Host "  ◆ Done! Type 'nexus' in Kitty to launch." -ForegroundColor Yellow
Write-Host ""