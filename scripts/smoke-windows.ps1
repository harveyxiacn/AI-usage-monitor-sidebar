<#
.SYNOPSIS
  Windows start-up smoke test (and optional upgrade smoke test).

.DESCRIPTION
  Launches the release exe and waits for `sidebar revealed` (plus
  `platform setup:` and `tray menu ready`) in
  %LOCALAPPDATA%\io.github.harveyxiacn.ai-usage-sidebar\logs\. Fails on a
  panic or an [ERROR] line. Meant for a CI runner: it kills the app by name and,
  in upgrade mode, installs the previous release and replaces settings.json.

.PARAMETER Exe          The freshly built ai-usage-sidebar.exe.
.PARAMETER UpgradeFrom  Optional: an OLDER release *_x64-setup.exe, installed
                        silently (per user) and started first. Then a BOM-prefixed
                        partial settings.json is seeded and -Exe starts on the
                        same profile.
.PARAMETER ExpectMigration  Old usage.db schema the new build must migrate from.
.PARAMETER LogCopy      Directory the logs are copied to on failure.
.PARAMETER Timeout      Seconds to wait for each marker (default 90).
#>
param(
  [Parameter(Mandatory = $true)][string]$Exe,
  [string]$UpgradeFrom = '',
  [string]$ExpectMigration = '',
  [string]$LogCopy = '',
  [int]$Timeout = 90
)
$ErrorActionPreference = 'Stop'

$AppId = 'io.github.harveyxiacn.ai-usage-sidebar'
$LogDir = Join-Path $env:LOCALAPPDATA "$AppId\logs"
$Settings = Join-Path $env:APPDATA "$AppId\settings.json"
$Work = Join-Path ([IO.Path]::GetTempPath()) ("smoke-" + [guid]::NewGuid())
New-Item -ItemType Directory -Force $Work | Out-Null
$Exe = (Resolve-Path $Exe).Path

if (-not $env:CI -and -not $env:SMOKE_ALLOW_REAL_PROFILE) {
  throw 'This test kills the app, clears its logs and rewrites settings.json; set SMOKE_ALLOW_REAL_PROFILE=1 to allow it.'
}

function Stop-App {
  # taskkill /T also takes the WebView2 children down with the app.
  Get-Process -Name 'ai-usage-sidebar' -ErrorAction SilentlyContinue | ForEach-Object {
    & taskkill.exe /PID $_.Id /T /F | Out-Null
  }
  for ($i = 0; $i -lt 20 -and (Get-Process -Name 'ai-usage-sidebar' -ErrorAction SilentlyContinue); $i++) {
    Start-Sleep -Milliseconds 250
  }
}

function Read-Logs {
  if (Test-Path $LogDir) {
    Get-ChildItem $LogDir -File -ErrorAction SilentlyContinue |
      ForEach-Object { Get-Content $_.FullName -ErrorAction SilentlyContinue }
  }
}

function Fail([string]$Message) {
  Write-Host "SMOKE FAILED: $Message"
  Write-Host '----- app log -----'
  $text = @(Read-Logs)
  if ($text.Count) { $text | ForEach-Object { Write-Host $_ } } else { Write-Host '(no log file was written)' }
  if ($LogCopy) {
    New-Item -ItemType Directory -Force $LogCopy | Out-Null
    if (Test-Path $LogDir) { Copy-Item "$LogDir\*" $LogCopy -Recurse -Force -ErrorAction SilentlyContinue }
  }
  Stop-App
  exit 1
}

function Test-Log([string]$Pattern) {
  [bool](@(Read-Logs) | Select-String -Pattern $Pattern -Quiet)
}

function Start-Phase([string]$Label, [string]$Path) {
  Write-Host "== ${Label}: launching $Path (timeout ${Timeout}s)"
  $proc = Start-Process -FilePath $Path -PassThru
  $deadline = (Get-Date).AddSeconds($Timeout)
  $found = $false
  while ((Get-Date) -lt $deadline) {
    if (Test-Log 'sidebar revealed') { $found = $true; break }
    if ($proc.HasExited -and -not (Get-Process -Name 'ai-usage-sidebar' -ErrorAction SilentlyContinue)) {
      Fail "${Label}: the app exited (code $($proc.ExitCode)) before it revealed the sidebar"
    }
    Start-Sleep -Milliseconds 500
  }
  if (-not $found) { Fail "${Label}: no 'sidebar revealed' in $LogDir within ${Timeout}s" }
  if (-not (Test-Log 'platform setup:')) { Fail "${Label}: no 'platform setup:' line" }
  if (-not (Test-Log 'tray menu ready')) { Fail "${Label}: no 'tray menu ready' line" }
  $bad = @(Read-Logs) | Select-String -Pattern 'panicked|\[ERROR\]'
  if ($bad) {
    $bad | ForEach-Object { Write-Host $_.Line }
    Fail "${Label}: the log holds a panic or an ERROR line (shown above)"
  }
  Write-Host "${Label}: start-up looks healthy:"
  @(Read-Logs) | Select-String -Pattern 'platform setup|tray menu ready|sidebar revealed|usage\.db schema' |
    ForEach-Object { Write-Host $_.Line }
}

try {
  Stop-App
  if (Test-Path $LogDir) { Remove-Item $LogDir -Recurse -Force }

  if ($UpgradeFrom) {
    $installer = (Resolve-Path $UpgradeFrom).Path
    Write-Host "Installing the previous release silently: $installer"
    $p = Start-Process -FilePath $installer -ArgumentList '/S' -PassThru -Wait
    if ($p.ExitCode -ne 0) { Fail "the previous release's installer exited with code $($p.ExitCode)" }
    # The installer may launch the app itself; start from a clean slate.
    Start-Sleep -Seconds 3
    Stop-App
    $old = Get-ChildItem -Path $env:LOCALAPPDATA, $env:ProgramFiles -Recurse -Filter 'ai-usage-sidebar.exe' -ErrorAction SilentlyContinue |
      Where-Object { $_.FullName -ne $Exe } | Select-Object -First 1
    if (-not $old) { Fail 'cannot find the installed previous release' }
    if (Test-Path $LogDir) { Remove-Item $LogDir -Recurse -Force }
    Start-Phase 'previous release' $old.FullName
    Stop-App
    # Keep the old run's log apart so the new run's assertions read only its own.
    Move-Item $LogDir (Join-Path $Work 'logs-previous')
    # A UTF-8 BOM in front of a partial key set: the shape of both v0.6 bugs.
    New-Item -ItemType Directory -Force (Split-Path $Settings) | Out-Null
    $json = '{ "edge": "left", "autoHide": true, "language": "zh-CN" }' + "`n"
    $bytes = [byte[]](0xEF, 0xBB, 0xBF) + [Text.UTF8Encoding]::new($false).GetBytes($json)
    [IO.File]::WriteAllBytes($Settings, $bytes)
  }

  Start-Phase 'this build' $Exe

  if ($UpgradeFrom) {
    if (-not (Test-Log 'platform setup:.*edge=Left.*autoHide=true')) {
      Fail 'the BOM-prefixed settings.json was not applied (expected edge=Left, autoHide=true)'
    }
    Write-Host 'BOM + partial settings.json applied.'
    if ($ExpectMigration) {
      if (-not (Test-Log "usage\.db schema $ExpectMigration -> ")) {
        Fail "expected the log line 'usage.db schema $ExpectMigration -> N'"
      }
      Write-Host "Database migration from schema $ExpectMigration logged."
    }
  }
  Write-Host 'Smoke test passed.'
} finally {
  Stop-App
  Remove-Item $Work -Recurse -Force -ErrorAction SilentlyContinue
}
