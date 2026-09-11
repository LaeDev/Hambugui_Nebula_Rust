$ErrorActionPreference = 'Stop'
. 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\gui-qa\env.ps1'

# §8.6's busy rule is global (`SheetData.busy`). Every mod job on this machine
# finishes in 1-6 s and a server duplicate of a sparse pad in 0.6 s, so this
# measurement needs both a slower job (2 GB of real bytes to copy) and a leaner
# probe: raw SendKeys with no settling sleep, and id lookups instead of
# Get-PageTitle's tree walk.
$pad = Join-Path $RootD 'servers\QAneo-1.21.1\files\pad.bin'
Start-Log $Shots 's10d-busy'
if (-not (Test-Path $pad)) {
    Say 'writing a 2 GB pad file'
    $fs = [System.IO.File]::Create($pad)
    $buf = New-Object byte[] (16MB)
    (New-Object Random 7).NextBytes($buf)
    for ($k = 0; $k -lt 128; $k++) { $fs.Write($buf, 0, $buf.Length) }
    $fs.Close()
}
Say ("pad: " + (Get-Item $pad).Length + " B")

$proc = Start-App 's10d'
try {
    Start-Sleep -Seconds 4
    Send-Keys '^2'
    Start-Sleep -Seconds 4
    [void](Invoke-Control (Find-ById 'servers-menu-QAneo-1.21.1'))
    Start-Sleep -Milliseconds 900
    [void](Invoke-Control (Find-ById 'server-menu-duplicate'))
    [void](Wait-Until { $null -ne (Find-ById 'server-duplicate-submit' -Optional -TimeoutSeconds 1) } 20 'dialog')
    Start-Sleep -Milliseconds 600
    Type-Text (Find-ById 'server-duplicate-id') 'QAbusy'
    Start-Sleep -Milliseconds 400
    [void](Activate-Win)

    $t0 = Get-Date
    [void](Invoke-Control (Find-ById 'server-duplicate-submit'))
    function Stamp { "{0,5:N1}s" -f ((Get-Date) - $t0).TotalSeconds }
    function DlgOpen { $null -ne (Find-ById 'server-duplicate-submit' -Optional -TimeoutSeconds 0) }

    $sub = Find-ById 'server-duplicate-submit' -Optional -TimeoutSeconds 0
    $a = @{ open = ($null -ne $sub); enabled = $(if ($sub) { $sub.Current.IsEnabled } else { $null }) }
    Say ("  [$(Stamp)] after submit: dialog=$($a.open) submit-enabled=$($a.enabled)")
    [void](Save-WindowShot (Join-Path $Shots 'mods-10-busy-dialog-duplicate.png'))
    $pill = @(Get-Texts) | Where-Object { $_ -like '*복제 중*' }
    Say ("  [$(Stamp)] pill: '" + (($pill | Select-Object -Unique) -join ' | ') + "'")

    [System.Windows.Forms.SendKeys]::SendWait('{ESC}')
    Start-Sleep -Milliseconds 120
    $openEsc = DlgOpen
    Say ("  [$(Stamp)] after Esc: dialog=$openEsc")

    $r = Get-WindowRects
    [void][NebulaWin32]::SetCursorPos($r.Left + 400, $r.Top + 130)
    [NebulaWin32]::mouse_event([NebulaWin32]::MOUSEEVENTF_LEFTDOWN, 0, 0, 0, [IntPtr]::Zero)
    [NebulaWin32]::mouse_event([NebulaWin32]::MOUSEEVENTF_LEFTUP, 0, 0, 0, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 150
    $openScrim = DlgOpen
    Say ("  [$(Stamp)] after a scrim click: dialog=$openScrim")

    [System.Windows.Forms.SendKeys]::SendWait('^1')
    Start-Sleep -Milliseconds 200
    $gen = Find-ById 'build-generate' -Optional -TimeoutSeconds 0
    if (-not $gen) { $gen = Find-ByPartialName '배포본 생성' 'Button' -Optional -TimeoutSeconds 1 }
    $genEnabled = $(if ($gen) { $gen.Current.IsEnabled } else { '<none>' })
    $openNav = DlgOpen
    Say ("  [$(Stamp)] after Ctrl+1: 빌드 primary='$(if ($gen) { $gen.Current.Name })' enabled=$genEnabled dialog=$openNav")
    [void](Save-WindowShot (Join-Path $Shots 'mods-10-busy-nav.png'))

    [System.Windows.Forms.SendKeys]::SendWait('^3')
    Start-Sleep -Milliseconds 200
    $upd = Find-ById 'mods-check-updates' -Optional -TimeoutSeconds 0
    $updEnabled = $(if ($upd) { $upd.Current.IsEnabled } else { '<none>' })
    Say ("  [$(Stamp)] on 모드: 업데이트 확인 enabled=$updEnabled dialog=$(DlgOpen)")
    [void](Save-WindowShot (Join-Path $Shots 'mods-10-busy-mods-page.png'))

    # When did busy actually stop?
    $stopped = $null
    for ($i = 0; $i -lt 400; $i++) {
        if (-not (DlgOpen)) { $stopped = (Get-Date) - $t0; break }
        Start-Sleep -Milliseconds 100
    }
    Say ("  job/dialog ended at {0:N1}s" -f $(if ($stopped) { $stopped.TotalSeconds } else { -1 }))
    Say ("  every probe above happened before that, so the busy state was live for all of them")

    Check 'the dialog stays open right after submit' $a.open
    Check 'its controls are disabled' ($a.enabled -eq $false)
    Check 'the top-bar pill names the job' ((($pill | Select-Object -Unique) -join ' ') -like '*복제 중*') (($pill | Select-Object -Unique) -join ' | ')
    Check 'Esc is refused while busy' $openEsc
    Check 'the scrim does not dismiss a busy dialog' $openScrim
    Check 'the 빌드 primary is disabled while a job runs' ($genEnabled -eq $false) "enabled=$genEnabled"
    Check 'the 모드 primary is disabled while a job runs' ($updEnabled -eq $false) "enabled=$updEnabled"
    Check 'the dialog survives navigating to another page and back' $openNav

    $toast = @()
    for ($i = 0; $i -lt 40 -and $toast.Count -eq 0; $i++) {
        $toast = @(Find-TextLike '복제했습니다') + @(Find-TextLike '복제 실패')
        if ($toast.Count -eq 0) { Start-Sleep -Milliseconds 250 }
    }
    Say ("  toast: " + (($toast | Select-Object -Unique) -join ' | '))
}
finally {
    Stop-App $proc
    Remove-Item -Recurse -Force (Join-Path $RootD 'servers\QAbusy-1.21.1') -ErrorAction SilentlyContinue
    Get-ChildItem (Join-Path $RootD 'servers') -Force -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -like '.*partial*' } | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -Force $pad -ErrorAction SilentlyContinue
    Say 'cleaned up QAbusy-1.21.1 and the pad file'
}
End-Log
