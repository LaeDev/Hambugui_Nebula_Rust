$ErrorActionPreference = 'Stop'
. 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\gui-qa\env.ps1'

# Every mod job on this machine finishes in 1-6 s, which is not enough time to
# walk Esc, the scrim and Ctrl+1 with the dialog still open. §8.6's busy rule is
# global (`SheetData.busy`), so measure it on a job that can be made long: a
# server duplicate over an 800 MB pad file.
$pad = Join-Path $RootD 'servers\QAneo-1.21.1\files\pad.bin'
Start-Log $Shots 's10b-busy'
if (-not (Test-Path $pad)) {
    Say 'creating an 800 MB pad file'
    $fs = [System.IO.File]::Create($pad)
    $fs.SetLength(800MB)
    $fs.Close()
}
Say ("pad: " + (Get-Item $pad).Length + " B")

$proc = Start-App 's10b'
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

    $t0 = Get-Date
    [void](Invoke-Control (Find-ById 'server-duplicate-submit'))
    function Snap([string] $what) {
        $d = Find-ById 'server-duplicate-submit' -Optional -TimeoutSeconds 0
        $f = Find-ById 'server-duplicate-id' -Optional -TimeoutSeconds 0
        $pill = (@(Find-TextLike '서버 복제 중')).Count -gt 0
        Say ("  [{0,5:N1}s] {1}: dialog={2} submit-enabled={3} field-enabled={4} pill={5}" -f `
            ((Get-Date) - $t0).TotalSeconds, $what, ($null -ne $d),
            $(if ($d) { $d.Current.IsEnabled } else { 'n/a' }),
            $(if ($f) { $f.Current.IsEnabled } else { 'n/a' }), $pill)
        [pscustomobject]@{ Open = ($null -ne $d); Enabled = $(if ($d) { $d.Current.IsEnabled } else { $null }); Pill = $pill }
    }

    $a = Snap 'right after submit'
    [void](Save-WindowShot (Join-Path $Shots 'mods-10-busy-dialog-duplicate.png'))
    Send-Keys '{ESC}' -NoClick
    $b = Snap 'after Esc'

    $r = Get-WindowRects
    [void][NebulaWin32]::SetCursorPos($r.Left + 400, $r.Top + 130)
    [NebulaWin32]::mouse_event([NebulaWin32]::MOUSEEVENTF_LEFTDOWN, 0, 0, 0, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 40
    [NebulaWin32]::mouse_event([NebulaWin32]::MOUSEEVENTF_LEFTUP, 0, 0, 0, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 300
    $c = Snap 'after a scrim click'

    Send-Keys '^1' -NoClick
    $title = Get-PageTitle
    $gen = Find-ById 'build-generate' -Optional -TimeoutSeconds 1
    if (-not $gen) { $gen = Find-ByPartialName '배포본 생성' 'Button' -Optional -TimeoutSeconds 1 }
    $genEnabled = if ($gen) { $gen.Current.IsEnabled } else { '<not found>' }
    Say ("  [{0,5:N1}s] after Ctrl+1: title='{1}' 빌드 primary='{2}' enabled={3}" -f `
        ((Get-Date) - $t0).TotalSeconds, $title, $(if ($gen) { $gen.Current.Name } else { '' }), $genEnabled)
    [void](Save-WindowShot (Join-Path $Shots 'mods-10-busy-nav.png'))

    Send-Keys '^3' -NoClick
    Start-Sleep -Milliseconds 300
    $upd = Find-ById 'mods-check-updates' -Optional -TimeoutSeconds 2
    $modsTitle = Get-PageTitle
    Say ("  [{0,5:N1}s] on 모드: title='{1}' 업데이트 확인 enabled={2}" -f `
        ((Get-Date) - $t0).TotalSeconds, $modsTitle, $(if ($upd) { $upd.Current.IsEnabled } else { '<none>' }))
    [void](Save-WindowShot (Join-Path $Shots 'mods-10-busy-mods-page.png'))
    $d = Snap 'back after navigating'

    Check 'the dialog stays open right after submit' $a.Open
    Check 'its controls are disabled' (($a.Enabled -eq $false) -or ($b.Enabled -eq $false))
    Check 'the top-bar pill names the job' ($a.Pill -or $b.Pill)
    Check 'Esc is refused while busy' $b.Open
    Check 'the scrim does not dismiss a busy dialog' $c.Open
    Check 'navigation still works while busy' ($title -eq '빌드') "title=$title"
    Check 'the 빌드 primary is disabled while a job runs' ($genEnabled -eq $false) "enabled=$genEnabled"
    Check 'the 모드 page primary is disabled while a job runs' ($upd -and -not $upd.Current.IsEnabled) ("enabled=" + $(if ($upd) { $upd.Current.IsEnabled } else { 'n/a' }))
    Check 'the dialog survives the navigation' $d.Open

    [void](Wait-Until { $null -eq (Find-ById 'server-duplicate-submit' -Optional -TimeoutSeconds 1) } 600 'job end')
    Say ("  job took {0:N1}s" -f ((Get-Date) - $t0).TotalSeconds)
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
    Remove-Item -Force $pad -ErrorAction SilentlyContinue
    Say 'cleaned up QAbusy-1.21.1 and the pad file'
}
End-Log
