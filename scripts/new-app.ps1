<#
.SYNOPSIS
  Make a new uchmk app from ito's templates/app.

.DESCRIPTION
  Copies templates/app to the destination, fills in the app's name, pins every
  ito crate to one rev, and makes the first build ready (git init, the RAM disk
  build folder, the shared lanes scripts, the bug form, Cargo.lock). It commits
  nothing. The app starts with the ito crates every uchmk app has; what is still
  to wire is in its TODO.md, and scripts/ito-first.py keeps it on ito's crates.

    pwsh -NoProfile -File scripts/new-app.ps1 -Name fuda -Description "TODO list"

.PARAMETER Name
  Lower-case app name (letters, digits, hyphens); it becomes the crate, the
  binary and the repository name (uchmk/<Name>).
.PARAMETER Description
  One line: Cargo.toml, README and --help.
.PARAMETER Dest
  Where to make it. Default: C:\dev\<Name> (a source folder is never on R:).
.PARAMETER Rev
  The ito commit to pin. Default: this checkout's HEAD, which must be pushed.
.PARAMETER NoBuild
  Stop after writing the files (no cargo, no lanes sync).
#>
param(
    [Parameter(Mandatory)][ValidatePattern('^[a-z][a-z0-9-]*$')][string]$Name,
    [Parameter(Mandatory)][string]$Description,
    [string]$Dest = "C:\dev\$Name",
    [string]$Rev,
    [switch]$NoBuild
)
$ErrorActionPreference = 'Stop'

$ito = Split-Path -Parent $PSScriptRoot
$template = Join-Path $ito 'templates\app'
if (-not (Test-Path $template)) { throw "no template at $template" }
if ((Test-Path $Dest) -and (Get-ChildItem $Dest -Force | Select-Object -First 1)) { throw "$Dest exists and is not empty" }

if (-not $Rev) {
    $Rev = (git -C $ito rev-parse HEAD).Trim()
    $remote = git -C $ito branch -r --contains $Rev 2>$null
    if (-not $remote) { Write-Warning "ito HEAD $Rev is not pushed: cargo cannot fetch it. Push ito first, or pass -Rev." }
}
$tokens = [ordered]@{
    '{{app}}'         = $Name
    '{{env_prefix}}' = $Name.ToUpperInvariant().Replace('-', '_')
    '{{rev}}'         = $Rev
    '{{description}}' = $Description
    '{{date}}'        = (Get-Date).ToString('yyyy-MM-dd')
}
$utf8 = New-Object System.Text.UTF8Encoding($false)
$root = (Resolve-Path $template).Path

Get-ChildItem $template -Recurse -Force -File | ForEach-Object {
    $rel = $_.FullName.Substring($root.Length + 1)
    $rel = ($rel -split '\\' | ForEach-Object { $_ -replace '^APP(-com\.rs)?$', ($Name + '$1') }) -join '\'
    # Cargo.toml is stored as Cargo.toml.tmpl: cargo reads every Cargo.toml in a git dependency, and the placeholder name is not a package name.
    $rel = $rel -replace '\.tmpl$', ''
    $to = Join-Path $Dest $rel
    New-Item -ItemType Directory -Force (Split-Path $to) | Out-Null
    $bytes = [IO.File]::ReadAllBytes($_.FullName)
    if ($_.Extension -in '.png', '.ico') { [IO.File]::WriteAllBytes($to, $bytes); return }
    $text = $utf8.GetString($bytes)
    foreach ($k in $tokens.Keys) { $text = $text.Replace($k, $tokens[$k]) }
    [IO.File]::WriteAllText($to, $text, $utf8)
}
Write-Host "wrote $Dest"
if ($NoBuild) { return }

Push-Location $Dest
try {
    git init -q -b main
    pwsh -NoProfile -File "$env:LOCALAPPDATA\uchmk-build\cargo-ramdisk.ps1"
    # The shared scripts: lanes.sh first (it copies the rest), from this checkout.
    Copy-Item (Join-Path $ito 'scripts\lanes\lanes.sh') scripts\lanes.sh
    $env:ITO_DIR = $ito
    # Git's bash, not the first `bash` on PATH: that is System32's WSL launcher, which fails where no distro is installed.
    $gitBash = Join-Path (Split-Path -Parent (Split-Path -Parent (Get-Command git).Source)) 'bin\bash.exe'
    if (-not (Test-Path $gitBash)) { $gitBash = 'bash' }
    & $gitBash scripts/lanes.sh sync
    if ($LASTEXITCODE) { throw 'lanes.sh sync failed' }
    Remove-Item Env:\ITO_DIR
    # The bug form is ito's shared one; the example writes it.
    New-Item -ItemType Directory -Force .github\ISSUE_TEMPLATE | Out-Null
    New-Item -ItemType File -Force .github\ISSUE_TEMPLATE\bug_report.yml | Out-Null
    cargo run -q -p $Name --example make-bugform
    if ($LASTEXITCODE) { throw 'make-bugform failed' }
    cargo run -q -p $Name --example make-testcheck
    if ($LASTEXITCODE) { throw 'make-testcheck failed' }
    cargo fmt --all
    python3 scripts/ito-first.py
    if ($LASTEXITCODE) { throw 'ito-first failed' }
    # Windows' core.filemode is false, so git would record the scripts as 100644 and CI could not run them.
    foreach ($sh in Get-ChildItem scripts -Filter *.sh) { git update-index --add --chmod=+x "scripts/$($sh.Name)" }
}
finally { Pop-Location }
Write-Host "`nDone. Nothing is committed. Next: read $Dest\TODO.md, run scripts/verify.sh, create uchmk/$Name."
# The policy file sends people to the Security tab, so these four must be on once the repository exists.
Write-Host "`nAfter creating uchmk/$Name on GitHub, turn on its security settings (as in the other apps):"
Write-Host "  gh api -X PUT repos/uchmk/$Name/vulnerability-alerts"
Write-Host "  gh api -X PUT repos/uchmk/$Name/automated-security-fixes"
Write-Host "  gh api -X PUT repos/uchmk/$Name/private-vulnerability-reporting"
Write-Host "  gh api -X PATCH repos/uchmk/$Name/code-scanning/default-setup -f state=configured -f 'languages[]=actions'"
