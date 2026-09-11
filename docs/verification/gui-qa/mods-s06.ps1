$ErrorActionPreference = 'Stop'
. 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\gui-qa\env.ps1'

Start-Log $Shots 's06-updates'
$proc = Start-App 's06'
try {
    Start-Sleep -Seconds 4
    Send-Keys '^3'
    Start-Sleep -Seconds 5

    # ── 7 (tail): the 교체 label, now that an older fabric-api is installed ──
    Say '--- 7 교체 label for a newer version ---'
    $row = Find-ById 'mods-row-fabric-api-0.92.11+1.20.1.jar' -Optional -TimeoutSeconds 4
    if ($row) {
        [void](Invoke-Control $row)
        Start-Sleep -Seconds 1
        [void](Invoke-Control (Find-ById 'mod-detail-versions'))
        [void](Wait-Until { (Find-ByIdPrefix 'version-row-').Count -gt 0 } 40 'versions')
        Start-Sleep -Seconds 1
        $vr = @(Find-ByIdPrefix 'version-row-')
        Say ("  version rows: " + (($vr | ForEach-Object { "'$($_.Current.Name)' enabled=$($_.Current.IsEnabled)" }) -join ' ; '))
        Check 'a newer version offers 교체' ((@($vr | Where-Object { $_.Current.Name -like '*0.92.12*교체' -and $_.Current.Name -notlike '*이전 버전으로 교체' })).Count -gt 0) (($vr | ForEach-Object { $_.Current.Name }) -join ' | ')
        Check 'the installed one is 설치됨' ((@($vr | Where-Object { $_.Current.Name -like '*0.92.11*설치됨' })).Count -gt 0)
        [void](Save-WindowShot (Join-Path $Shots 'mods-07-version-actions.png'))
        [void](Invoke-Control (Find-ById 'version-picker-cancel'))
        Start-Sleep -Milliseconds 800
        $c = Find-ById 'mod-detail-cancel' -Optional -TimeoutSeconds 2
        if ($c) { [void](Invoke-Control $c); Start-Sleep -Milliseconds 600 }
    } else { Say '  !! fabric-api 0.92.11 row not found' }

    # ── 6 update check ─────────────────────────────────────────────────────
    Say '--- 6 업데이트 확인 ---'
    Send-Keys '^{HOME}' -NoClick
    Start-Sleep -Milliseconds 500
    $chk = Find-ById 'mods-check-updates'
    Say ("  button: '$($chk.Current.Name)'  header: " + ((Find-TextLike '관리 ') -join ' | '))
    [void](Invoke-Control $chk)
    $toast = @()
    for ($i = 0; $i -lt 80 -and $toast.Count -eq 0; $i++) {
        $toast = @(Find-TextLike '업데이트 ') | Where-Object { $_ -like '*개 있음' }
        $toast += @(Find-TextLike '모두 최신입니다')
        if ($toast.Count -eq 0) { Start-Sleep -Milliseconds 250 }
    }
    Say ("  toast: " + (($toast | Select-Object -Unique) -join ' | '))
    Check '업데이트 확인 reports a count' ($toast.Count -gt 0) (($toast | Select-Object -Unique) -join ' | ')
    Start-Sleep -Seconds 2
    Say ("  header after the check: " + ((Find-TextLike '관리 ') -join ' | '))
    Check 'the header caption records the check' (((Find-TextLike '관리 ') -join ' ') -notlike '*확인 전*') ((Find-TextLike '관리 ') -join ' | ')
    Check 'the 업데이트 가능 chip is on the row' (Has-Text '업데이트 가능')
    [void](Save-WindowShot (Join-Path $Shots 'mods-06-check.png'))

    $all = Find-ById 'mods-update-all' -Optional -TimeoutSeconds 3
    Check '모두 업데이트 (N) appears when N > 0' ($null -ne $all)
    if ($all) {
        Say ("  primary: '$($all.Current.Name)'")
        [void](Invoke-Control $all)
        Check 'update-preview opened' (Wait-Until { $null -ne (Find-ById 'update-preview-submit' -Optional -TimeoutSeconds 1) } 30 'preview')
        Start-Sleep -Seconds 1
        Say '  --- update-preview texts ---'
        foreach ($t in (Get-Texts)) { Say ("    " + $t) }
        $sub = Find-ById 'update-preview-submit'
        Say ("  buttons: submit='$($sub.Current.Name)' cancel='$((Find-ById 'update-preview-cancel').Current.Name)'")
        Say ("  initial focus: " + (Focused-Desc))
        [void](Save-WindowShot (Join-Path $Shots 'mods-06-update-preview.png'))

        $t0 = Get-Date
        [void](Invoke-Control $sub)
        $seenPill = $false; $seenProgress = $false; $dialogOpen = $false
        for ($i = 0; $i -lt 60; $i++) {
            if ($null -ne (Find-ById 'update-preview-submit' -Optional -TimeoutSeconds 0)) { $dialogOpen = $true }
            if ((@(Find-TextLike '모드 업데이트 중')).Count -gt 0) { $seenPill = $true }
            if ($null -ne (Find-ById 'mods-installed-progress' -Optional -TimeoutSeconds 0)) { $seenProgress = $true }
            if ($i -gt 4 -and $null -eq (Find-ById 'update-preview-submit' -Optional -TimeoutSeconds 0)) { break }
            Start-Sleep -Milliseconds 120
        }
        Say ("  during apply: dialog-open=$dialogOpen pill=$seenPill ProgressRow(설치됨)=$seenProgress  {0:N1}s" -f ((Get-Date) - $t0).TotalSeconds)
        Check 'the update pill names the job' $seenPill
        Check 'the ProgressRow sits in the 설치됨 card' $seenProgress
        $t2 = @()
        for ($i = 0; $i -lt 60 -and $t2.Count -eq 0; $i++) {
            $t2 = @(Get-Texts) | Where-Object { $_ -like '*업데이트*했습니다*' -or $_ -like '*업데이트 실패*' }
            if ($t2.Count -eq 0) { Start-Sleep -Milliseconds 250 }
        }
        Say ("  toast: " + (($t2 | Select-Object -Unique) -join ' | '))
        [void](Save-WindowShot (Join-Path $Shots 'mods-06-updated.png'))
    }

    Start-Sleep -Seconds 4
    Send-Keys '^{HOME}' -NoClick
    Start-Sleep -Seconds 2
    Say '  --- list after the update ---'
    foreach ($t in (Get-Texts)) { Say ("    " + $t) }
    Say ("  disk: " + ((Get-ChildItem (Join-Path $RootD 'servers\QAfab-1.20.1\fabricmods\required') | ForEach-Object { $_.Name }) -join ', '))

    Say '--- 6 second 업데이트 확인 ---'
    [void](Invoke-Control (Find-ById 'mods-check-updates'))
    $t3 = @()
    for ($i = 0; $i -lt 80 -and $t3.Count -eq 0; $i++) {
        $t3 = @(Find-TextLike '모두 최신입니다') + (@(Find-TextLike '업데이트 ') | Where-Object { $_ -like '*개 있음' })
        if ($t3.Count -eq 0) { Start-Sleep -Milliseconds 250 }
    }
    Say ("  toast: " + (($t3 | Select-Object -Unique) -join ' | '))
    Check 'a second check reports 모두 최신입니다' ((($t3 | Select-Object -Unique) -join ' ') -like '*모두 최신입니다*') (($t3 | Select-Object -Unique) -join ' | ')
    [void](Save-WindowShot (Join-Path $Shots 'mods-06-uptodate.png'))
}
finally { Stop-App $proc }
End-Log
