$ErrorActionPreference = 'Stop'
. 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\gui-qa\env.ps1'

Start-Log $Shots 's07-version-picker'
$proc = Start-App 's07'
try {
    Start-Sleep -Seconds 4
    Send-Keys '^3'
    Start-Sleep -Seconds 5

    Say '--- 7 version-picker on Fabric API ---'
    $file = 'fabric-api-0.92.12+1.20.1.jar'
    $row = Find-ById "mods-row-$file" -Optional -TimeoutSeconds 5
    if (-not $row) {
        for ($i = 0; $i -lt 8 -and -not $row; $i++) { Scroll-Page 3; $row = Find-ById "mods-row-$file" -Optional -TimeoutSeconds 1 }
    }
    Check 'the Fabric API row is reachable' ($null -ne $row)
    [void](Invoke-Control $row)
    Start-Sleep -Seconds 1
    Say ("  detail ids: " + ((Get-Ids | Where-Object { $_ -like 'mod-detail*' }) -join ' ; '))
    Check '다른 버전… is offered for a managed mod' ($null -ne (Find-ById 'mod-detail-versions' -Optional -TimeoutSeconds 3))
    Check '프로젝트 페이지 열기 is offered for a managed mod' ($null -ne (Find-ById 'mod-detail-project' -Optional -TimeoutSeconds 2))
    [void](Save-WindowShot (Join-Path $Shots 'mods-07-detail-managed.png'))

    [void](Invoke-Control (Find-ById 'mod-detail-versions'))
    Start-Sleep -Seconds 1
    Check 'the version picker opened' (Wait-Until { (Find-ByIdPrefix 'version-row-').Count -gt 0 } 40 'versions')
    Start-Sleep -Seconds 1
    Say '  --- dialog texts ---'
    foreach ($t in (Get-Texts)) { Say ("    " + $t) }
    $rows = @(Find-ByIdPrefix 'version-row-')
    Say ("  version rows: " + (($rows | ForEach-Object { "$($_.Current.AutomationId)|'$($_.Current.Name)'|enabled=$($_.Current.IsEnabled)" }) -join ' ; '))
    [void](Save-WindowShot (Join-Path $Shots 'mods-07-version-picker.png'))

    $labels = @($rows | ForEach-Object { $_.Current.Name })
    Check 'the installed version is a disabled 설치됨' ((@($rows | Where-Object { $_.Current.Name -like '*설치됨' -and -not $_.Current.IsEnabled }).Count) -gt 0) ($labels -join ' | ')
    Check 'an older version offers 이전 버전으로 교체' ((@($labels | Where-Object { $_ -like '*이전 버전으로 교체' }).Count) -gt 0) ($labels -join ' | ')
    Say ("  교체 rows: " + ((@($labels | Where-Object { $_ -like '*교체' -and $_ -notlike '*이전 버전으로 교체' })) -join ' | '))

    # ── downgrade: two clicks on the same row (phase C note) ───────────────
    $old = $rows | Where-Object { $_.Current.Name -like '*이전 버전으로 교체' -and $_.Current.IsEnabled } | Select-Object -First 1
    Check 'a downgrade target exists' ($null -ne $old)
    if ($old) {
        $oldId = $old.Current.AutomationId
        Say ("  picking: $oldId  '$($old.Current.Name)'")
        [void](Invoke-Control $old)
        Start-Sleep -Seconds 1
        $line = @(Find-TextLike '로 내립니다')
        Say ("  confirm line: " + ($line -join ' | '))
        Check 'the downgrade asks a one-line confirmation inside the dialog' ($line.Count -gt 0) ($line -join ' | ')
        [void](Save-WindowShot (Join-Path $Shots 'mods-07-downgrade-confirm.png'))

        $again = Find-ById $oldId
        $t0 = Get-Date
        [void](Invoke-Control $again)
        $seenPill = $false
        for ($i = 0; $i -lt 50; $i++) {
            if ((@(Find-TextLike '모드 교체 중')).Count -gt 0 -or (@(Find-TextLike '모드 설치 중')).Count -gt 0) { $seenPill = $true }
            if ($null -eq (Find-ById 'version-picker-cancel' -Optional -TimeoutSeconds 0) -and $i -gt 3) { break }
            Start-Sleep -Milliseconds 150
        }
        Say ("  pill seen during the replace: $seenPill")
        $toast = @()
        for ($i = 0; $i -lt 40 -and $toast.Count -eq 0; $i++) {
            $toast = @(Find-TextLike '교체')  | Where-Object { $_ -like '*했습니다' -or $_ -like '*실패' }
            if ($toast.Count -eq 0) { Start-Sleep -Milliseconds 250 }
        }
        Say ("  toast: " + (($toast | Select-Object -Unique) -join ' | '))
        Say ("  replace took {0:N1}s" -f ((Get-Date) - $t0).TotalSeconds)
        [void](Save-WindowShot (Join-Path $Shots 'mods-07-downgraded.png'))
        Start-Sleep -Seconds 4
        Send-Keys '^3' -NoClick
        Start-Sleep -Seconds 3
        Say '  --- list after the downgrade ---'
        foreach ($t in (Get-Texts)) { Say ("    " + $t) }
        $ns = Join-Path $RootD 'servers\QAfab-1.20.1\fabricmods\required'
        Say ("  disk required: " + ((Get-ChildItem $ns | ForEach-Object { $_.Name }) -join ', '))
        Check 'exactly one fabric-api jar is on disk' ((@(Get-ChildItem $ns -Filter 'fabric-api*.jar')).Count -eq 1) ((Get-ChildItem $ns -Filter 'fabric-api*.jar' | ForEach-Object { $_.Name }) -join ', ')
    }
}
finally { Stop-App $proc }
End-Log
