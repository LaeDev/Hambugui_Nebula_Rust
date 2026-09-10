# dump-tree.ps1 — launch the built GUI, dump its accessibility tree, close it.
#
# The evidence behind `gui-v2/README.md` §9: it prints every node with its
# AutomationId and counts how many carry one, so a control added without an
# `accessible-id` shows up as a number that did not go up.
#
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -STA -File dump-tree.ps1 `
#       -Exe target\release\nebula.exe -Out docs\verification\gui-qa\tree-after.txt
#
# This file must stay UTF-8 **with BOM**, like `drive.ps1`: Windows PowerShell
# 5.1 reads a BOM-less UTF-8 file as ANSI and every Korean label breaks.
param(
    [string] $Exe = 'target\release\nebula.exe',
    [string] $Out = 'docs\verification\gui-qa\tree-after.txt',
    [string] $Root,
    [int] $SettleSeconds = 6
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

. (Join-Path $PSScriptRoot 'drive.ps1')

# `Dump-Tree` joins a relative path onto its own shot directory, so resolve it
# here and hand it an absolute one.
$Out = [System.IO.Path]::GetFullPath((Join-Path (Get-Location) $Out))

# A throwaway config dir, so a dump can never touch real settings.
$config = Join-Path ([System.IO.Path]::GetTempPath()) ("nebula-tree-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $config | Out-Null
if (-not $Root) {
    $Root = Join-Path $config 'root'
    New-Item -ItemType Directory -Force -Path $Root | Out-Null
}

$env:NEBULA_CONFIG_DIR = $config
$env:ROOT = $Root
$env:BASE_URL = 'https://cdn.example.invalid/'
$env:RUST_LOG = 'info'

Start-Log (Split-Path -Parent $Out) 'dump-tree'
Say "exe  : $Exe"
Say "root : $Root"

$proc = Start-Process -FilePath $Exe -ArgumentList 'gui' -PassThru
try {
    Connect-App -ProcessId $proc.Id | Out-Null
    # The first scan lands on the first tick; give it room before reading.
    Start-Sleep -Seconds $SettleSeconds
    $nodes = Dump-Tree $Out

    $lines = Get-Content -LiteralPath $Out -Encoding UTF8
    $withId = @($lines | Where-Object { $_ -notmatch "AutomationId=''" })
    Say ("nodes: {0}" -f $nodes)
    Say ("AutomationIds: {0}" -f $withId.Count)
    Say '--- ids ---'
    foreach ($line in $withId) {
        if ($line -match "AutomationId='([^']+)'") { Say ("  " + $Matches[1]) }
    }

    # D11: the root FocusScope used to sit in the cycle as `Window / Nebula`,
    # a stop with no focus ring anywhere on screen. It should not appear.
    Say '--- tab order ---'
    Activate-Win | Out-Null
    $seen = @()
    for ($i = 0; $i -lt 12; $i++) {
        Send-Keys '{TAB}' -NoClick
        Start-Sleep -Milliseconds 180
        $focused = Get-Focused
        if ($focused) {
            $c = $focused.Current
            $seen += "$($c.ControlType.ProgrammaticName.Replace('ControlType.','')) / $($c.Name) / $($c.AutomationId)"
            Say ("  {0,2}: {1}" -f $i, $seen[-1])
        }
    }
    Check 'the root FocusScope is not a tab stop' (-not ($seen -match '^Window / Nebula'))

    # Taking the root scope out of the Tab cycle means nothing hands focus back
    # to it. Closing a dialog destroys the field that had it, so the close paths
    # re-focus the scope by hand — if they did not, Ctrl+1..5 would go dead and
    # only a click would revive them. This is that path, end to end.
    Say '--- keyboard after a dialog closes ---'
    Check 'reached the 서버 page' (Goto-Page '서버')
    [void](Invoke-Control (Find-Control '새 서버 만들기' 'Button'))
    Start-Sleep -Milliseconds 400
    $dialog = Find-Control '새 서버' 'Group' -Optional
    Check 'the 새 서버 dialog opened' ($null -ne $dialog)
    Check 'the caret starts in the first field' `
        ((Get-Focused).Current.AutomationId -eq 'new-server-id') `
        (Get-Focused).Current.AutomationId

    Send-Keys '{ESC}'
    Start-Sleep -Milliseconds 400
    Check 'Esc closed it' ($null -eq (Find-Control '새 서버' 'Group' -Optional))

    Send-Keys '^1'
    Start-Sleep -Milliseconds 400
    $title = Get-PageTitle
    Check 'Ctrl+1 still switches pages afterwards' ($title -eq '빌드') "title=$title"

    # Two more things this run can settle, both about state the eye cannot
    # check from a screenshot.
    #
    # D2's mitigation: PageDown must reach content the mouse wheel would have
    # to scroll to, and that content must then appear in the tree at all.
    #
    # Review item 5: `SegmentedControl` no longer writes its own `selected`, so
    # the 테마 segment follows `SettingsData.theme` however the theme changed —
    # including from the top bar's toggle, which never touches the segment. The
    # old self-assignment destroyed the `selected: theme-index` binding on the
    # first click, after which the two could disagree for good.
    Say '--- PageDown reaches the 모양 card, and the theme segment follows ---'
    Check 'reached the 설정 page' (Goto-Page '설정')
    Activate-Win | Out-Null
    $before = ((Get-Win).FindAll($script:Scope::Descendants,
        (New-Object System.Windows.Automation.PropertyCondition(
            $script:AE::IsOffscreenProperty, $false)))).Count
    for ($i = 0; $i -lt 4; $i++) { Send-Keys '{PGDN}' -NoClick; Start-Sleep -Milliseconds 250 }
    Start-Sleep -Milliseconds 500

    function Read-Segment {
        (@('테마 시스템', '테마 라이트', '테마 다크') | ForEach-Object {
            $el = Find-Control $_ 'TabItem' -Optional -TimeoutSeconds 2
            if ($el) { "$_=$(Get-Checked $el)" }
        }) -join '  '
    }

    $segment = Read-Segment
    Say "  before: $segment"
    Check 'PageDown brought the 모양 card into the tree' ($segment -ne '') "was $before nodes"

    [void](Invoke-Control (Find-ByPartialName '테마로 전환' 'Button'))
    Start-Sleep -Milliseconds 700
    $moved = Read-Segment
    Say "  after : $moved"
    Check 'the top-bar toggle moves the theme segment' ($moved -ne $segment) "$segment -> $moved"
}
finally {
    Close-AppWindow
    Start-Sleep -Seconds 2
    if (-not $proc.HasExited) { $proc.Kill() }
    Remove-Item -Recurse -Force $config -ErrorAction SilentlyContinue
}
End-Log
