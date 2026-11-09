# Source is uchmk/ito scripts/lanes/; an app's copy is written by its scripts/lanes.sh sync, so edit ito's.
<#
Run the app once with `--keys` and print what it wrote -- the Windows lanes' way of
pressing keys and reading the result (kura: .claude/windows-role.md). Linux has
scripts/xrun.sh for the same job. The app is scripts\lanes.conf's `bin`, and the
variable it writes when the keys are done is its `keys_env` (kura: KURA_KEYS_DONE).

  scripts\keys.ps1 -Dir C:\fixtures -Keys '<Tab>C<State:spot><Quit>' [-Out R:\run1] [-Launches] [-Isolate]
  scripts\keys.ps1 -Dir C:\fixtures -Keys '<Quit>' -Evidence -Label smoke [-Spec F]... [-Scenario F]... [-EvidenceRoot D]
  scripts\keys.ps1 -Foreground

Gives the run its own folder (the keys_env file, `<State:name>` and `<Shot:name>`
files all land there), starts the app, waits for it, then prints the done file and
every `*.txt` beside it, one block per file. Returns the folder's path.

A reused -Out is first emptied of the last run's files (the done file, `*.txt`,
`*.png`, `*.json`, `*.jsonl` (trace.jsonl), `isolate\`): a `<State:>` or `<Shot:>` this run never wrote was
printed and read as this run's. A folder this script did not make (no `.keys-run`
and no `keys.done` in it) is refused, exit 4, so nothing of anyone else's is deleted.

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

  -Evidence   an evidence run (docs/visual-test.md): this script calls `evidence.py new` for the folder (-Out and
              -Launches are refused, exit 5), sets the run-id variable (keys_env without `_DONE`, plus `_RUN_ID`),
              implies -Isolate (so isolate= must be set), puts the app in a Windows job object (what it starts from
              then on is in the job too), and on EVERY ending (done, failed, timeout, a stopped or crashed script)
              removes `isolate\`, stops what the app left behind (named in the manifest's note), and calls
              `evidence.py manifest` with the exit code, `--killed` when the app had to be stopped (a timeout, a
              stopped script) and the cleanup verdict (PASS only when the app was in the job, the job was empty and
              `isolate\` was removed). -Label names the run, each -Spec goes to `new` and
              `manifest`, each -Scenario to `manifest`, -EvidenceRoot overrides lanes.conf's `evidence=`. The manifest's
              problem lines are printed after the file blocks and the folder is the last line.

The variables this script sets (the keys_env one, the isolate ones, and in -Evidence the run-id one) get back their
previous values, or are removed if they were not set, when it returns, in every mode. Without -Evidence the run-id
variable is never touched, so one set by hand still reaches the app.

A system dialog in front while the app runs is written to `blocked.txt` in the run's
folder and makes the run no result (exit 3): its keys may have gone to the dialog.

End the keys with `<Quit>` where `q` means something else (kura#236). A done file
whose last line is not `keys: done` (`keys: stalled`, `keys: refused`, `keys: failed`
for a step the app could not carry out or a file it could not write, `keys: unsettled`
for a key that went in before the app had settled) is not a result: say so in the
report with its lines.

Exit code: 0 for a result; 1 when the done file is missing or does not end with
`keys: done`; 2 when the app outlived -Timeout; 3 when a system dialog was in front;
4 when -Out is a folder this script did not make. The output is printed either way.
With -Evidence: 5 when no evidence was made (a refusal before `new`, `new` failed, `manifest` could not run or
exited 2), 6 when the run's own code was 0 but the manifest lists problems; otherwise the run's code 1, 2 or 3
(the manifest's problems are expected then and do not change it). TODO "keys.ps1 / xrun.sh -Evidence", #205.
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
    [Parameter(ParameterSetName = 'Run')][switch]$Evidence,
    [Parameter(ParameterSetName = 'Run')][string]$Label = 'keys',
    [Parameter(ParameterSetName = 'Run')][string[]]$Spec = @(),
    [Parameter(ParameterSetName = 'Run')][string[]]$Scenario = @(),
    [Parameter(ParameterSetName = 'Run')][string]$EvidenceRoot,
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

# A Windows job object: -Evidence puts the app in one, so what it started later can be listed and ended.
if (-not ('LaneJob' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class LaneJob {
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern IntPtr CreateJobObjectW(IntPtr attrs, string name);
    [DllImport("kernel32.dll")] static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll")] static extern bool QueryInformationJobObject(IntPtr job, int cls, IntPtr info, int len, IntPtr ret);
    [DllImport("kernel32.dll")] static extern bool TerminateJobObject(IntPtr job, uint code);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);

    public static IntPtr Create() { return CreateJobObjectW(IntPtr.Zero, null); }
    public static bool Assign(IntPtr job, IntPtr process) { return AssignProcessToJobObject(job, process); }
    public static bool Terminate(IntPtr job) { return TerminateJobObject(job, 1); }
    public static void Close(IntPtr job) { CloseHandle(job); }

    // The ids of the processes in the job (JobObjectBasicProcessIdList = 3: two DWORDs, then ULONG_PTR ids).
    public static int[] Pids(IntPtr job) {
        const int max = 256;
        int size = 8 + IntPtr.Size * max;
        IntPtr buf = Marshal.AllocHGlobal(size);
        try {
            // A failed query (also: more than 256 processes) is reported as one id of -1, so it is never read as an empty job.
            if (!QueryInformationJobObject(job, 3, buf, size, IntPtr.Zero)) { return new int[] { -1 }; }
            int n = Marshal.ReadInt32(buf, 4);
            var ids = new int[n];
            for (int i = 0; i < n; i++) { ids[i] = (int)Marshal.ReadIntPtr(buf, 8 + i * IntPtr.Size).ToInt64(); }
            return ids;
        } finally { Marshal.FreeHGlobal(buf); }
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

if ($Evidence -and $Out) {
    Write-Warning 'keys.ps1: -Evidence makes the folder itself (evidence.py new); do not pass -Out'
    exit 5
}
if ($Evidence -and $Launches) {
    Write-Warning 'keys.ps1: -Evidence cannot be used with -Launches: an evidence run must leave nothing running'
    exit 5
}
$conf = @{}
foreach ($line in Get-Content -LiteralPath (Join-Path $PSScriptRoot 'lanes.conf')) {
    if ($line -match '^([a-z_]+)=(.*)$') { $conf[$Matches[1]] = $Matches[2] }
}
# A refusal before anything is made: exit 5 ("no evidence") in an evidence run, a thrown error otherwise.
function Stop-Refused {
    param([string]$Why)
    if ($Evidence) { Write-Warning "keys.ps1: -Evidence refused: $Why"; exit 5 }
    throw "keys.ps1: $Why"
}
$name = $conf['bin']
if (-not $name -or -not $conf['keys_env']) { Stop-Refused 'scripts\lanes.conf needs bin= and keys_env=' }
if (-not $Bin) { $Bin = Join-Path (Split-Path $PSScriptRoot -Parent) "target\release\$name.exe" }
if (-not (Test-Path -LiteralPath $Bin)) { Stop-Refused "$Bin does not exist (cargo build --release, or pass -Bin)" }

# Evidence: the refusals come before anything is made, so a refused run leaves no folder behind.
$py = $null
$runEnv = $null
if ($Evidence) {
    $why = $null
    if (-not $conf['keys_env'].EndsWith('_DONE')) { $why = 'keys_env in scripts\lanes.conf does not end in _DONE, so there is no run-id variable name' }
    elseif (-not (($conf['isolate'] -split '\s+') | Where-Object { $_ })) { $why = '-Evidence implies -Isolate, and scripts\lanes.conf has no isolate=' }
    else {
        foreach ($cand in @(@('py', '-3'), @('python3'), @('python'))) {
            try {
                $null = & $cand[0] @($cand | Select-Object -Skip 1) --version 2>&1
                if ($LASTEXITCODE -eq 0) { $py = $cand; break }
            } catch { }
        }
        if (-not $py) { $why = 'no Python found (tried py -3, python3, python)' }
    }
    if ($why) { Write-Warning "keys.ps1: -Evidence refused: $why"; exit 5 }
    $runEnv = $conf['keys_env'].Substring(0, $conf['keys_env'].Length - '_DONE'.Length) + '_RUN_ID'
    $Isolate = $true
}

# evidence.py is run as UTF-8 both ways, so a Japanese path or note survives the pipe.
function Invoke-Evidence {
    param([string[]]$EvArgs)
    $evpy = Join-Path $PSScriptRoot 'evidence.py'
    $prevEnc = [Console]::OutputEncoding
    $prevEap = $ErrorActionPreference
    try {
        [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
        $ErrorActionPreference = 'Continue'
        $lines = @(& $py[0] @($py | Select-Object -Skip 1) -X utf8 $evpy @EvArgs 2>&1 | ForEach-Object { "$_" })
        [pscustomobject]@{ Code = $LASTEXITCODE; Lines = $lines }
    } finally {
        $ErrorActionPreference = $prevEap
        [Console]::OutputEncoding = $prevEnc
    }
}

$rid = $null
if ($Evidence) {
    $newArgs = @('new', '--label', $Label)
    foreach ($s in $Spec) { $newArgs += @('--spec', $s) }
    if ($EvidenceRoot) { $newArgs += @('--evidence', $EvidenceRoot) }
    try { $r = Invoke-Evidence $newArgs }
    catch { Write-Warning "keys.ps1: evidence.py new could not run: $($_.Exception.Message)"; exit 5 }
    $kv = @{}
    foreach ($l in $r.Lines) { if ($l -match '^(run_id|dir|env|scale_env|scale)=(.*)$') { $kv[$Matches[1]] = $Matches[2] } }
    if ($r.Code -ne 0 -or -not $kv['run_id'] -or -not $kv['dir'] -or -not $kv['env']) {
        $r.Lines | ForEach-Object { Write-Warning "evidence.py: $_" }
        Write-Warning 'keys.ps1: evidence.py new did not make a run folder; nothing was run'
        exit 5
    }
    $rid = $kv['run_id']
    $Out = $kv['dir']
    $runEnv = $kv['env']
}
# The app's scale override (scale_env= in lanes.conf), evidence runs only. `new` hands over the first spec's
# pixels_per_point (source spec); with no spec the variable may already be set by hand (source environment).
# Either way it goes to the manifest, so a run on an overridden scale cannot pass for one on a real screen.
$scaleOverride = $null
if ($Evidence) {
    if ($kv['scale_env'] -and $kv['scale']) {
        $scaleOverride = @{ Env = $kv['scale_env']; Value = $kv['scale']; Source = 'spec' }
    } elseif ($conf['scale_env']) {
        $have = [Environment]::GetEnvironmentVariable($conf['scale_env'])
        if ($have) { $scaleOverride = @{ Env = $conf['scale_env']; Value = $have; Source = 'environment' } }
    }
}
if (-not $Out) { $Out = Join-Path ([IO.Path]::GetTempPath()) ("$name-keys-" + [guid]::NewGuid().ToString('N').Substring(0, 8)) }
$done = Join-Path $Out 'keys.done'
$marker = Join-Path $Out '.keys-run'

# Every variable this script sets goes back to what it was (or is removed), so a second run in one session
# does not inherit the first one's isolate folders or done file.
$savedEnv = [ordered]@{}
function Set-LaneEnv {
    param([string]$Name, [string]$Value)
    if (-not $savedEnv.Contains($Name)) { $savedEnv[$Name] = [Environment]::GetEnvironmentVariable($Name) }
    [Environment]::SetEnvironmentVariable($Name, $Value)
}

# One argument for the Windows command line. Backslashes before a quote or at the end are doubled, or they would escape the
# quote: `"C:\"` does not end there, and the arguments after it run together.
function ConvertTo-LaneArg {
    param([string]$Value)
    '"' + (($Value -replace '(\\*)"', '$1$1\"') -replace '(\\+)\z', '$1$1') + '"'
}

$code = 0
$exitCode = $null
$notes = New-Object System.Collections.Generic.List[string]
$timedOut = $false
$job = [IntPtr]::Zero
$jobOk = $false
$p = $null
$rethrow = $null
$isolateGone = $true
$left = @()
$finished = $false
$problems = @()
try {
    try {
        if (-not $Evidence) {
            if ((Test-Path -LiteralPath $Out) -and -not (Test-Path -LiteralPath $marker) -and -not (Test-Path -LiteralPath $done) -and
                (Get-ChildItem -LiteralPath $Out -Force | Select-Object -First 1)) {
                Write-Warning "keys.ps1: -Out $Out has files this script did not write; pass a new or empty folder"
                exit 4
            }
            New-Item -ItemType Directory -Force -Path $Out | Out-Null
            Set-Content -LiteralPath $marker -Value "a keys.ps1 run folder: each run empties it of the last run's files" -Encoding utf8
            # A file that cannot be removed stops the run (ErrorActionPreference): it would be read as this run's.
            Get-ChildItem -LiteralPath $Out -File | Where-Object { $_.Name -like 'keys.done*' -or $_.Name -eq 'run.json.part' -or $_.Extension -in '.txt', '.png', '.json', '.jsonl' } |
                Remove-Item -Force
            # The shots (`shots\<name>.png` and `.json`) and the `<State:>` files live in subfolders: empty them too. Something else
            # there (a link, a file) is not ours to delete, and is not a folder this run can write into: stop.
            foreach ($sub in 'shots', 'state') {
                $sp = Join-Path $Out $sub
                $item = Get-Item -LiteralPath $sp -Force -ErrorAction SilentlyContinue
                if (-not $item) { continue }
                if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                    throw "keys.ps1: $sp is a link or not a folder; remove it, or pass a new -Out"
                }
                # A link further down (a junction) could lead a recursive delete out of the folder: refuse it too.
                $link = Get-ChildItem -LiteralPath $sp -Recurse -Force -Attributes ReparsePoint -ErrorAction Stop | Select-Object -First 1
                if ($link) { throw "keys.ps1: $($link.FullName) is a link inside $sp; remove it, or pass a new -Out" }
                Remove-Item -LiteralPath $sp -Recurse -Force -ErrorAction Stop
            }
            if (Test-Path -LiteralPath (Join-Path $Out 'isolate')) { Remove-Item -LiteralPath (Join-Path $Out 'isolate') -Recurse -Force }
        }
        # In -Evidence the folder is new and evidence.py wrote .keys-run and pre\: nothing above runs, so nothing of theirs is touched.
        Set-LaneEnv $conf['keys_env'] $done
        if ($Evidence) { Set-LaneEnv $runEnv $rid }
        if ($Evidence -and $scaleOverride -and $scaleOverride.Source -eq 'spec') { Set-LaneEnv $scaleOverride.Env $scaleOverride.Value }
        if ($Isolate) {
            $vars = @(($conf['isolate'] -split '\s+') | Where-Object { $_ })
            if (-not $vars) { throw 'keys.ps1: -Isolate needs isolate= in scripts\lanes.conf' }
            foreach ($v in $vars) {
                $home_ = Join-Path $Out "isolate\$v"
                New-Item -ItemType Directory -Force -Path $home_ | Out-Null
                Set-LaneEnv $v $home_
            }
        }

        # One argument list, quoted by us: a `<Wait:1500>` or a space-bearing path survives.
        $argList = @((ConvertTo-LaneArg $(if ($Dir.Length -gt 3) { $Dir.TrimEnd('\') } else { $Dir })), '--keys', (ConvertTo-LaneArg $Keys))
        $p = Start-Process -FilePath $Bin -ArgumentList $argList -PassThru
        if ($Evidence) {
            # A child the app starts between Start-Process and the assignment escapes the job; the app starts
            # nothing before its window exists, which is later than this.
            $job = [LaneJob]::Create()
            $jobOk = ($job -ne [IntPtr]::Zero) -and [LaneJob]::Assign($job, $p.Handle)
            if (-not $jobOk) {
                $notes.Add("job object assignment failed; only the app's own process can be ended, and the cleanup cannot be judged")
            }
        }
        $deadline = (Get-Date).AddSeconds($Timeout)
        $blocked = $null
        while (-not $p.WaitForExit(500)) {
            if (-not $blocked) { $blocked = Get-Dialog }
            if ((Get-Date) -gt $deadline) {
                Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
                $timedOut = $true
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
        $finished = $true
    } catch {
        $code = 1
        $notes.Add("launcher error: $($_.Exception.Message)")
        Write-Warning "keys.ps1: launcher error: $($_.Exception.Message)"
        if (-not $Evidence) { $rethrow = $_ }
    } finally {
        if ($Evidence) {
            if (-not $finished -and $code -eq 0) {
                # Stopped from outside (Ctrl+C): the run did not finish, whatever the app had done so far.
                $code = 1
                $notes.Add('the run was interrupted before it finished')
            }
            # Whatever the app left running (or the kill left) is named, ended, and fails the cleanup.
            $alive = {
                if ($jobOk) { @([LaneJob]::Pids($job)) }
                elseif ($p -and -not $p.HasExited) { @($p.Id) }
                else { @() }
            }
            if ($timedOut) {
                # The kill is asynchronous: give the app's own process a moment to leave the job before judging.
                for ($i = 0; $i -lt 15 -and @(& $alive).Count -gt 0; $i++) { Start-Sleep -Milliseconds 200 }
            }
            $left = @(& $alive)
            if ($left.Count -gt 0) {
                $names = foreach ($id in $left) {
                    if ($id -lt 0) { 'unknown (the job could not be listed)'; continue }
                    $pr = Get-Process -Id $id -ErrorAction SilentlyContinue
                    "$(if ($pr) { $pr.ProcessName } else { '?' }) ($id)"
                }
                $notes.Add('left running after the run: ' + ($names -join ', '))
                if ($jobOk) { [void][LaneJob]::Terminate($job) }
                else { foreach ($id in $left) { Stop-Process -Id $id -Force -ErrorAction SilentlyContinue } }
                for ($i = 0; $i -lt 25 -and @(& $alive).Count -gt 0; $i++) { Start-Sleep -Milliseconds 200 }
                if (@(& $alive).Count -gt 0) { $notes.Add('still running after the job was terminated: ' + (@(& $alive) -join ', ')) }
            }
            if ($job -ne [IntPtr]::Zero) { [LaneJob]::Close($job) }
            $iso = Join-Path $Out 'isolate'
            try {
                $item = Get-Item -LiteralPath $iso -Force -ErrorAction SilentlyContinue
                if ($item) {
                    if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw "$iso is a link or not a folder" }
                    Remove-Item -LiteralPath $iso -Recurse -Force -ErrorAction Stop
                }
            } catch {
                $isolateGone = $false
                $notes.Add("isolate was not removed: $($_.Exception.Message)")
            }

            # Here, not after this `finally`: a stopped script (Ctrl+C) runs `finally` and nothing after it.
            # Nothing is written to the pipeline below (that throws while stopping); the problem lines are printed at the end.
            $judged = (-not $p) -or $jobOk
            $cleanup = if ($judged -and $left.Count -eq 0 -and $isolateGone -and -not ($notes -like 'still running*')) { 'PASS' } else { 'FAIL' }
            $mArgs = @('manifest', '--run', $Out, '--exit-code', "$code", '--cleanup', $cleanup)
            if ($timedOut -or ($p -and $left -contains $p.Id)) { $mArgs += '--killed' }
            if ($notes.Count -gt 0) { $mArgs += @('--note', ($notes -join '; ')) }
            if ($scaleOverride) { $mArgs += @('--scale-env', $scaleOverride.Env, "--scale-value=$($scaleOverride.Value)", '--scale-source', $scaleOverride.Source) }
            foreach ($s in $Spec) { $mArgs += @('--spec', $s) }
            foreach ($s in $Scenario) { $mArgs += @('--scenario', $s) }
            try {
                $m = Invoke-Evidence $mArgs
                $problems = @($m.Lines | Where-Object { $_ -like 'problem:*' })
                if ($m.Code -eq 0) { $exitCode = $code }
                elseif ($m.Code -eq 1) { $exitCode = if ($code -eq 0) { 6 } else { $code } }
                else {
                    $exitCode = 5
                    $m.Lines | ForEach-Object { Write-Warning "evidence.py: $_" }
                    Write-Warning "keys.ps1: evidence.py manifest exited $($m.Code); there is no manifest"
                }
            } catch {
                $exitCode = 5
                Write-Warning "keys.ps1: evidence.py manifest could not run: $($_.Exception.Message)"
            }
        }
    }
} finally {
    foreach ($k in $savedEnv.Keys) { [Environment]::SetEnvironmentVariable($k, $savedEnv[$k]) }
}
if ($rethrow) { throw $rethrow }
$problems | ForEach-Object { $_ }
if ($null -eq $exitCode) { $exitCode = $code }
$Out
exit $exitCode
