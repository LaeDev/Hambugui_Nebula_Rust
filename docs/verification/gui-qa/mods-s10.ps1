$ErrorActionPreference = 'Stop'
. 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\gui-qa\env.ps1'

# s05's install finished in 1.3-2.6 s, too fast to photograph the busy state.
# Pick a plan big enough to still be running while the probe walks Esc, the
# scrim and Ctrl+1.
Start-Log $Shots 's10-busy'
$proc = Start-App 's10'
try {
    Start-Sleep -Seconds 4
    Send-Keys '^3'
    Start-Sleep -Seconds 4
    [void](Invoke-Control (Find-ById 'mods-view-1'))
    Start-Sleep -Milliseconds 800

    $picked = $null
    foreach ($q in 'distant horizons', 'iris', 'create fabric', 'sodium') {
        Type-Text (Find-ById 'mods-search') $q
        Send-Keys '{ENTER}'
        Start-Sleep -Seconds 5
        foreach ($b in @(Find-ByIdPrefix 'mods-install-' | Where-Object { $_.Current.IsEnabled })) {
            $id = $b.Current.AutomationId
            [void](Invoke-Control $b)
            if (-not (Wait-Until { $null -ne (Find-ById 'install-confirm-submit' -Optional -TimeoutSeconds 1) } 30 'dialog')) { continue }
            Start-Sleep -Milliseconds 800
            $footer = @(Find-TextLike '개 · ') | Select-Object -First 1
            Say ("  candidate $q / $id -> footer '$footer'")
            $mb = 0.0
            if ($footer -match '([\d.]+)\s*MB') { $mb = [double]$Matches[1] }
            if ($mb -ge 4.0) { $picked = $id; break }
            [void](Invoke-Control (Find-ById 'install-confirm-cancel'))
            Start-Sleep -Milliseconds 600
        }
        if ($picked) { break }
    }
    Check 'found a plan big enough to photograph' ($null -ne $picked) "picked=$picked"
    if (-not $picked) { return }

    Say "--- 10 busy model on $picked ---"
    $t0 = Get-Date
    [void](Invoke-Control (Find-ById 'install-confirm-submit'))
    $log = New-Object System.Collections.Generic.List[string]
    function Note([string] $what) {
        $d = Find-ById 'install-confirm-submit' -Optional -TimeoutSeconds 0
        $pill = (@(Find-TextLike '모드 설치 중')).Count -gt 0
        $pr = Find-ById 'mods-search-progress' -Optional -TimeoutSeconds 0
        $line = ("  [{0,5:N1}s] {1}: dialog={2} submit-enabled={3} pill={4} progress='{5}'" -f `
            ((Get-Date) - $t0).TotalSeconds, $what, ($null -ne $d),
            $(if ($d) { $d.Current.IsEnabled } else { 'n/a' }), $pill,
            $(if ($pr) { $pr.Current.Name } else { '' }))
        Say $line
        $log.Add($line)
        $null -ne $d
    }

    $open0 = Note 'right after submit'
    [void](Save-WindowShot (Join-Path $Shots 'mods-10-busy-dialog.png'))
    Send-Keys '{ESC}' -NoClick
    $openEsc = Note 'after Esc'

    $r = Get-WindowRects
    [void][NebulaWin32]::SetCursorPos($r.Left + 400, $r.Top + 140)
    [NebulaWin32]::mouse_event([NebulaWin32]::MOUSEEVENTF_LEFTDOWN, 0, 0, 0, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 40
    [NebulaWin32]::mouse_event([NebulaWin32]::MOUSEEVENTF_LEFTUP, 0, 0, 0, [IntPtr]::Zero)
    $openScrim = Note 'after a scrim click'

    Send-Keys '^1' -NoClick
    $title = Get-PageTitle
    $gen = Find-ById 'build-generate' -Optional -TimeoutSeconds 1
    if (-not $gen) { $gen = Find-ByPartialName '배포본 생성' 'Button' -Optional -TimeoutSeconds 1 }
    $genEnabled = if ($gen) { $gen.Current.IsEnabled } else { '<not found>' }
    Say ("  [{0,5:N1}s] after Ctrl+1: title='{1}' 빌드 primary='{2}' enabled={3}" -f `
        ((Get-Date) - $t0).TotalSeconds, $title, $(if ($gen) { $gen.Current.Name } else { '' }), $genEnabled)
    [void](Save-WindowShot (Join-Path $Shots 'mods-10-busy-nav.png'))
    $openNav = Note 'after Ctrl+1'

    Send-Keys '^3' -NoClick
    $pr = Find-ById 'mods-search-progress' -Optional -TimeoutSeconds 1
    if ($pr) {
        Say ("  ProgressRow: role=$($pr.Current.ControlType.ProgrammaticName) name='$($pr.Current.Name)'")
        [void](Save-WindowShot (Join-Path $Shots 'mods-05-progress.png'))
    }
    [void](Note 'back on 모드')

    Check 'the dialog stays open right after submit' $open0
    Check 'Esc is refused while busy' $openEsc
    Check 'the scrim does not dismiss a busy dialog' $openScrim
    Check 'navigation still works while busy' ($title -eq '빌드') "title=$title"
    Check 'the 빌드 primary is disabled while a mod job runs' ($genEnabled -eq $false) "enabled=$genEnabled"
    Check 'the dialog is still open after navigating away and back' $openNav

    [void](Wait-Until { $null -eq (Find-ById 'install-confirm-submit' -Optional -TimeoutSeconds 1) } 300 'job end')
    Say ("  job took {0:N1}s" -f ((Get-Date) - $t0).TotalSeconds)
    $toast = @()
    for ($i = 0; $i -lt 40 -and $toast.Count -eq 0; $i++) {
        $toast = @(Find-TextLike '설치했습니다') + @(Find-TextLike '설치 실패')
        if ($toast.Count -eq 0) { Start-Sleep -Milliseconds 250 }
    }
    Say ("  toast: " + (($toast | Select-Object -Unique) -join ' | '))
    [void](Save-WindowShot (Join-Path $Shots 'mods-10-done.png'))
}
finally { Stop-App $proc }
End-Log
