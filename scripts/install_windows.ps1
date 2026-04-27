# install_windows.ps1 — build release binaries and install `cg` + `code-grasp-mcp` to your PATH (Windows).
#
# Run from PowerShell in the repo root, e.g.:
#   pwsh -ExecutionPolicy Bypass -File .\scripts\install_windows.ps1
#
# MCP is stdio-based: your editor spawns `code-grasp-mcp` as a subprocess.
# Indexes live at "<project>\.code-grasp\" (vectors + SQLite).

$ErrorActionPreference = 'Stop'

if ($env:OS -ne 'Windows_NT') {
    Write-Error 'This script is for Windows only (expected OS=Windows_NT).'
}

if ($PSVersionTable.PSVersion.Major -lt 5) {
    Write-Error "PowerShell 5.1 or newer is required."
}

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$BinDir = if ($env:BIN_DIR) { $env:BIN_DIR } else { Join-Path $env:USERPROFILE '.local\bin' }

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Error "cargo not found. Install Rust from https://rustup.rs and open a new terminal."
}

New-Item -ItemType Directory -Force -Path $BinDir | Out-Null

Write-Host "==> building release from $RepoRoot"
Push-Location $RepoRoot
try {
    if ($env:CODEGRASP_ORT_DYNAMIC -eq '1') {
        Write-Host '    CODEGRASP_ORT_DYNAMIC=1: build uses system ONNX (ort load-dynamic). Set ORT_DYLIB_PATH to onnxruntime.dll compatible with ort 2.0 / ONNX Runtime 1.20+.'
        cargo build --release --no-default-features --features ort-dynamic -p code-grasp -p code-grasp-mcp
    }
    else {
        cargo build --release -p code-grasp -p code-grasp-mcp
    }
}
finally {
    Pop-Location
}

$CgSrc = Join-Path $RepoRoot 'target\release\cg.exe'
$McpSrc = Join-Path $RepoRoot 'target\release\code-grasp-mcp.exe'
if (-not (Test-Path -LiteralPath $CgSrc)) { Write-Error "missing $CgSrc — build failed?" }
if (-not (Test-Path -LiteralPath $McpSrc)) { Write-Error "missing $McpSrc — build failed?" }

Write-Host "==> installing to $BinDir"
Copy-Item -LiteralPath $CgSrc -Destination (Join-Path $BinDir 'cg.exe') -Force
Copy-Item -LiteralPath $McpSrc -Destination (Join-Path $BinDir 'code-grasp-mcp.exe') -Force

Write-Host ''
Write-Host 'Installed:'
Write-Host "  $(Join-Path $BinDir 'cg.exe')"
Write-Host "  $(Join-Path $BinDir 'code-grasp-mcp.exe')"
Write-Host ''
Write-Host 'Add to PATH if needed (User environment variables), e.g.:'
Write-Host "  $BinDir"
Write-Host ''
Write-Host 'MCP: configure your client to run:'
Write-Host "  $(Join-Path $BinDir 'code-grasp-mcp.exe')"
Write-Host ''
Write-Host 'Tip: after git pull, run this script again so PATH binaries match the tree.'
Write-Host 'Optional NVIDIA GPU on Windows: build with --features ort-cuda (see README).'
