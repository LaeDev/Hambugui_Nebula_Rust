$ErrorActionPreference = 'Stop'
. 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\gui-qa\env.ps1'

Start-Log $Shots 's08-mod-delete'
$proc = Start-App 's08'
try {
    Start-Sleep -Seconds 4
    Send-Keys '^3'
    Start-Sleep -Seconds 5

    Say '--- 8 mod-delete with dependents ---'
    $file = 'fabric-api-0.92.12+1.20.1.jar'
    $row = Find-ById "mods-row-$file" -Optional -TimeoutSeconds 5
    Check 'the Fabric API row is present' ($null -ne $row)
    [void](Invoke-Control $row)
    Start-Sleep -Seconds 1
    [void](Invoke-Control (Find-ById 'mod-detail-delete'))
    Check 'mod-delete opened' (Wait-Until { $null -ne (Find-ById 'mod-delete-submit' -Optional -TimeoutSeconds 1) } 20 'dialog')
    Start-Sleep -Milliseconds 800

    Say '  --- dialog texts ---'
    foreach ($t in (Get-Texts)) { Say ("    " + $t) }
    Say ("  ids: " + ((Get-Ids | Where-Object { $_ -like 'mod-delete*' }) -join ' ; '))
    Say ("  initial focus: " + (Focused-Desc))
    Check '취소 has the initial focus' ((Focused-Id) -eq 'mod-delete-cancel') (Focused-Id)
    Check 'the title names the file' (Has-Text "모드 삭제 — $file")
    Check 'the body gives the absolute path' ((@(Find-TextLike '이 파일이 영구 삭제됩니다')).Count -gt 0) ((Find-TextLike '이 파일이 영구 삭제됩니다') -join ' | ')
    $dep = @(Find-TextLike '를 필요로 하는 모드')
    Say ("  dependents warning: " + ($dep -join ' | '))
    Check 'the dependents warning names Sodium Extra' ((($dep -join ' ') -like '*Sodium Extra*')) ($dep -join ' | ')
    $sub = Find-ById 'mod-delete-submit'
    Say ("  buttons: submit='$($sub.Current.Name)' cancel='$((Find-ById 'mod-delete-cancel').Current.Name)'")
    [void](Save-WindowShot (Join-Path $Shots 'mods-08-delete-warning.png'))

    $t0 = Get-Date
    [void](Invoke-Control $sub)
    $seenPill = $false
    for ($i = 0; $i -lt 60; $i++) {
        if ((@(Find-TextLike '모드 삭제 중')).Count -gt 0) { $seenPill = $true }
        if ($i -gt 3 -and $null -eq (Find-ById 'mod-delete-submit' -Optional -TimeoutSeconds 0)) { break }
        Start-Sleep -Milliseconds 120
    }
    $toast = @()
    for ($i = 0; $i -lt 60 -and $toast.Count -eq 0; $i++) {
        $toast = @(Find-TextLike '삭제했습니다') + @(Find-TextLike '삭제 실패')
        if ($toast.Count -eq 0) { Start-Sleep -Milliseconds 250 }
    }
    Say ("  pill seen: $seenPill   toast: " + (($toast | Select-Object -Unique) -join ' | ') + ("   {0:N1}s" -f ((Get-Date) - $t0).TotalSeconds))
    Check 'the delete pill names the job' $seenPill
    [void](Save-WindowShot (Join-Path $Shots 'mods-08-deleted.png'))

    Start-Sleep -Seconds 4
    Send-Keys '^{HOME}' -NoClick
    Start-Sleep -Seconds 2
    Say '  --- list after the delete ---'
    foreach ($t in (Get-Texts)) { Say ("    " + $t) }
    $left = @(Get-ChildItem (Join-Path $RootD 'servers\QAfab-1.20.1\fabricmods\required') | ForEach-Object { $_.Name })
    Say ("  disk required: " + ($left -join ', '))
    Check 'the jar is gone from disk' ($left -notcontains $file) ($left -join ', ')
    Check 'the row is gone from the list' ($null -eq (Find-ById "mods-row-$file" -Optional -TimeoutSeconds 2))

    # ── 취소 path, on a mod with no dependents ─────────────────────────────
    Say '--- 8 취소 leaves the file alone ---'
    $f2 = 'sodium-extra-0.5.9+mc1.20.1.jar'
    $r2 = Find-ById "mods-row-$f2" -Optional -TimeoutSeconds 4
    if ($r2) {
        [void](Invoke-Control $r2)
        Start-Sleep -Milliseconds 800
        [void](Invoke-Control (Find-ById 'mod-detail-delete'))
        Start-Sleep -Seconds 1
        Say ("  dependents warning for a leaf mod: " + ((Find-TextLike '를 필요로 하는 모드') -join ' | '))
        [void](Invoke-Control (Find-ById 'mod-delete-cancel'))
        Start-Sleep -Seconds 1
        $still = @(Get-ChildItem (Join-Path $RootD 'servers\QAfab-1.20.1\fabricmods\required') | ForEach-Object { $_.Name })
        Check '취소 deleted nothing' ($still -contains $f2) ($still -join ', ')
    }
}
finally { Stop-App $proc }
End-Log
