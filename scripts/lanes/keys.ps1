# Source is uchmk/ito scripts/lanes/; an app's copy is written by its scripts/lanes.sh sync, so edit ito's.
<#
Run the app once with `--keys` and print what it wrote -- the Windows lanes' way of
pressing keys and reading the result (kura: .claude/windows-role.md). Linux has
scripts/xrun.sh for the same job. The app is scripts\lanes.conf's `bin`, and the
variable it writes when the keys are done is its `keys_env` (kura: KURA_KEYS_DONE).

  scripts\keys.ps1 -Dir C:\fixtures -Keys '<Tab>C<State:spot><Quit>' [-Out R:\run1] [-Launches]

Gives the run its own folder (the keys_env file, `<State:name>` and `<Shot:name>`
files all land there), starts the app, waits for it, then prints the done file and
every `*.txt` beside it, one block per file. Returns the folder's path.

  -Launches   the keys start a program (sections 22, 26, 32, 37, `<F12>`). `-Wait`
              waits for descendants too and would never return while the editor or
              browser is open (kura#234), so this waits on the app's own id instead.
  -Timeout    seconds to wait for the app (default 120); a run that outlives it is
              stopped and reported as timed out, not as a result.
  -Bin        the binary (default target\release\<bin>.exe beside this script's repo).
              `-Kura` is the same, the name it had in kura.

End the keys with `<Quit>` where `q` means something else (kura#236). A done file
whose last line is not `keys: done` (`keys: stalled`, `keys: refused`) is not a
result: say so in the report with its lines.

Exit code: 0 for a result; 1 when the done file is missing or does not end with
`keys: done`; 2 when the app outlived -Timeout. The output is printed either way.
#>
param(
    [Parameter(Mandatory)][string]$Dir,
    [Parameter(Mandatory)][string]$Keys,
    [string]$Out,
    [Alias('Kura')][string]$Bin,
    [int]$Timeout = 120,
    [switch]$Launches
)
$ErrorActionPreference = 'Stop'
$conf = @{}
foreach ($line in Get-Content -LiteralPath (Join-Path $PSScriptRoot 'lanes.conf')) {
    if ($line -match '^([a-z_]+)=(.*)$') { $conf[$Matches[1]] = $Matches[2] }
}
$name = $conf['bin']
if (-not $name -or -not $conf['keys_env']) { throw 'keys.ps1: scripts\lanes.conf needs bin= and keys_env=' }
if (-not $Bin) { $Bin = Join-Path (Split-Path $PSScriptRoot -Parent) "target\release\$name.exe" }
if (-not (Test-Path -LiteralPath $Bin)) { throw "keys.ps1: $Bin does not exist (cargo build --release, or pass -Bin)" }
if (-not $Out) { $Out = Join-Path ([IO.Path]::GetTempPath()) ("$name-keys-" + [guid]::NewGuid().ToString('N').Substring(0, 8)) }
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$done = Join-Path $Out 'keys.done'
Remove-Item -LiteralPath $done -ErrorAction SilentlyContinue
[Environment]::SetEnvironmentVariable($conf['keys_env'], $done)
$code = 0

# One argument list, quoted by us: a `<Wait:1500>` or a space-bearing path survives.
$argList = @('"' + $(if ($Dir.Length -gt 3) { $Dir.TrimEnd('\') } else { $Dir }) + '"', '--keys', ('"' + $Keys.Replace('"', '\"') + '"'))
$p = Start-Process -FilePath $Bin -ArgumentList $argList -PassThru
if (-not $p.WaitForExit($Timeout * 1000)) {
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    $code = 2
    Write-Warning "keys.ps1: $name was still running after $Timeout s and was stopped; what follows is NOT a result"
}
# -Launches: the run may have started programs; the app's own exit is all we wait for
# (Start-Process -Wait would wait for them too).
if (-not $Launches) { Start-Sleep -Milliseconds 300 }

if (-not (Test-Path -LiteralPath $done)) {
    if ($code -eq 0) { $code = 1 }
    Write-Warning "keys.ps1: no done file -- $name vanished before writing one; not a result"
} else {
    $last = (Get-Content -LiteralPath $done | Select-Object -Last 1)
    if ($last -ne 'keys: done') { if ($code -eq 0) { $code = 1 }; Write-Warning "keys.ps1: the done file ends with '$last', not 'keys: done'; not a result" }
}
foreach ($f in @($done) + @(Get-ChildItem -LiteralPath $Out -Filter *.txt | Sort-Object Name | ForEach-Object FullName)) {
    if (Test-Path -LiteralPath $f) {
        "--- $(Split-Path $f -Leaf)"
        Get-Content -LiteralPath $f
    }
}
$Out
exit $code
