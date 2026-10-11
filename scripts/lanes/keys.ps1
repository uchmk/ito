# Source is uchmk/ito scripts/lanes/; an app's copy is written by its scripts/lanes.sh sync, so edit ito's.
<#
Run the app once with `--keys` and print what it wrote -- the Windows lanes' way of
pressing keys and reading the result (kura: .claude/windows-role.md). Linux has
scripts/xrun.sh for the same job. The app is scripts\lanes.conf's `bin`, and the
variable it writes when the keys are done is its `keys_env` (kura: KURA_KEYS_DONE).

  scripts\keys.ps1 -Dir C:\fixtures -Keys '<Tab>C<State:spot><Quit>' [-Out R:\run1] [-Launches] [-Isolate]
  scripts\keys.ps1 -Foreground

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
  -Isolate    the run reads and writes no settings of the machine's: each variable in
              scripts\lanes.conf's `isolate=` (space-separated; kura: its config and
              state homes) points at its own empty folder under the run's folder.
  -Foreground only says whether a system dialog is in front (a firewall or security
              prompt): one line, exit 0 when none, 3 when one is. Run it before a row
              that clicks or drags: synthetic mouse input does not reach the app while
              such a dialog is up (kura#327).

A system dialog in front while the app runs is written to `blocked.txt` in the run's
folder and makes the run no result (exit 3): its keys may have gone to the dialog.

End the keys with `<Quit>` where `q` means something else (kura#236). A done file
whose last line is not `keys: done` (`keys: stalled`, `keys: refused`) is not a
result: say so in the report with its lines.

Exit code: 0 for a result; 1 when the done file is missing or does not end with
`keys: done`; 2 when the app outlived -Timeout; 3 when a system dialog was in front.
The output is printed either way.
#>
[CmdletBinding(DefaultParameterSetName = 'Run')]
param(
    [Parameter(Mandatory, ParameterSetName = 'Run')][string]$Dir,
    [Parameter(Mandatory, ParameterSetName = 'Run')][string]$Keys,
    [Parameter(ParameterSetName = 'Run')][string]$Out,
    [Parameter(ParameterSetName = 'Run')][Alias('Kura')][string]$Bin,
    [Parameter(ParameterSetName = 'Run')][int]$Timeout = 120,
    [Parameter(ParameterSetName = 'Run')][switch]$Launches,
    [Parameter(ParameterSetName = 'Run')][switch]$Isolate,
    [Parameter(Mandatory, ParameterSetName = 'Check')][switch]$Foreground
)
$ErrorActionPreference = 'Stop'

if (-not ('LaneForeground' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class LaneForeground {
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassNameW(IntPtr hwnd, StringBuilder name, int max);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowTextW(IntPtr hwnd, StringBuilder text, int max);

    // The foreground window's class and title, "" when there is none.
    public static string[] Window() {
        var hwnd = GetForegroundWindow();
        if (hwnd == IntPtr.Zero) { return new[] { "", "" }; }
        var cls = new StringBuilder(256);
        GetClassNameW(hwnd, cls, cls.Capacity);
        var title = new StringBuilder(512);
        GetWindowTextW(hwnd, title, title.Capacity);
        return new[] { cls.ToString(), title.ToString() };
    }
}
'@
}

# The classes of the shell's own prompts that take the keys and the mouse from
# every app: the firewall's and Windows Security's (kura#327).
$dialogs = @('Shell_SystemDialog', 'Shell_SystemDim', 'Credential Dialog Xaml Host')

# The system dialog in front, as "class 'title'"; $null when there is none.
function Get-Dialog {
    $w = [LaneForeground]::Window()
    if ($dialogs -contains $w[0]) { return "$($w[0]) '$($w[1])'" }
    $null
}

if ($Foreground) {
    $d = Get-Dialog
    if ($d) { "blocked by a system dialog: $d"; exit 3 }
    $w = [LaneForeground]::Window()
    "no system dialog in front (foreground: $($w[0]) '$($w[1])')"
    exit 0
}

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
Remove-Item -LiteralPath (Join-Path $Out 'blocked.txt') -ErrorAction SilentlyContinue
[Environment]::SetEnvironmentVariable($conf['keys_env'], $done)
if ($Isolate) {
    $vars = @(($conf['isolate'] -split '\s+') | Where-Object { $_ })
    if (-not $vars) { throw 'keys.ps1: -Isolate needs isolate= in scripts\lanes.conf' }
    foreach ($v in $vars) {
        $home_ = Join-Path $Out "isolate\$v"
        New-Item -ItemType Directory -Force -Path $home_ | Out-Null
        [Environment]::SetEnvironmentVariable($v, $home_)
    }
}
$code = 0

# One argument list, quoted by us: a `<Wait:1500>` or a space-bearing path survives.
$argList = @('"' + $(if ($Dir.Length -gt 3) { $Dir.TrimEnd('\') } else { $Dir }) + '"', '--keys', ('"' + $Keys.Replace('"', '\"') + '"'))
$p = Start-Process -FilePath $Bin -ArgumentList $argList -PassThru
$deadline = (Get-Date).AddSeconds($Timeout)
$blocked = $null
while (-not $p.WaitForExit(500)) {
    if (-not $blocked) { $blocked = Get-Dialog }
    if ((Get-Date) -gt $deadline) {
        Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
        $code = 2
        Write-Warning "keys.ps1: $name was still running after $Timeout s and was stopped; what follows is NOT a result"
        break
    }
}
if ($blocked) {
    Set-Content -LiteralPath (Join-Path $Out 'blocked.txt') -Value "blocked by a system dialog: $blocked" -Encoding utf8
    $code = 3
    Write-Warning "keys.ps1: a system dialog was in front ($blocked); its keys may have gone to it; NOT a result"
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
