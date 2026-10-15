$ErrorActionPreference = 'Stop'
. 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\cf-reqa\gui\cf-env.ps1'

Start-Log $Shots 'cf-reqa-03'
$proc = Start-App 'cf-reqa-03'
try {
    Start-Sleep -Seconds 4
    Send-Keys '^3'
    Start-Sleep -Seconds 3
    Say ("scope: " + ((Find-TextLike ' Minecraft ') -join ' | '))

    Say '--- 찾기 tab ---'
    [void](Invoke-Control (Find-ById 'mods-view-1'))
    Start-Sleep -Seconds 1

    Say '--- provider = CurseForge ---'
    $cf = Find-ById 'mods-provider-1'
    Say ("  segment: name='$($cf.Current.Name)' enabled=$($cf.Current.IsEnabled)")
    [void](Invoke-Control $cf)
    Start-Sleep -Milliseconds 600

    $field = Find-ById 'mods-search'
    for ($i = 0; $i -lt 6; $i++) {
        [void](Activate-Win)
        Type-Text $field 'jei'
        Start-Sleep -Milliseconds 300
        $v = Get-Value $field
        Say ("  attempt $i field value = '$v'")
        if ($v -eq 'jei') { break }
    }
    Check 'search field carries jei' ((Get-Value $field) -eq 'jei') ("value='" + (Get-Value $field) + "'")
    Send-Keys '{ENTER}'
    $got = Wait-Until { (Find-ByIdPrefix 'mods-install-').Count -gt 0 } 60 'results'
    Check 'CurseForge search returned rows' $got
    Start-Sleep -Seconds 2

    $btns = @(Find-ByIdPrefix 'mods-install-')
    Say ("  install buttons: " + (($btns | ForEach-Object { $_.Current.AutomationId }) -join ' ; '))
    Check 'results are CurseForge rows' (($btns | Where-Object { $_.Current.AutomationId -like 'mods-install-curseforge-*' }).Count -eq $btns.Count)
    [void](Save-WindowShot (Join-Path $Shots 'cf-reqa-03-search.png'))

    Say '  --- row captions (CF-02 GUI side) ---'
    foreach ($t in (Get-Texts)) { Say ("    " + $t) }

    $target = $btns | Where-Object { $_.Current.AutomationId -eq 'mods-install-curseforge-238222' } | Select-Object -First 1
    if (-not $target) { $target = $btns[0] }
    Say ("--- install-confirm for " + $target.Current.AutomationId + " ---")
    [void](Invoke-Control $target)
    $vbtn = Find-ById 'install-confirm-versions' -TimeoutSeconds 40
    Start-Sleep -Seconds 1
    foreach ($t in (Get-Texts)) { Say ("    " + $t) }
    [void](Save-WindowShot (Join-Path $Shots 'cf-reqa-03-confirm.png'))

    Say '--- 다른 버전... ---'
    Say ("  button name='$($vbtn.Current.Name)'")
    [void](Invoke-Control $vbtn)
    [void](Wait-Until { (Find-ByIdPrefix 'version-row-').Count -gt 0 } 40 'version rows')
    Start-Sleep -Seconds 2
    $rows = @(Find-ByIdPrefix 'version-row-')
    $ids = @($rows | ForEach-Object { $_.Current.AutomationId })
    Say ("  version rows: " + ($ids -join ' ; '))
    Say '  --- version picker texts ---'
    foreach ($t in (Get-Texts)) { Say ("    " + $t) }
    [void](Save-WindowShot (Join-Path $Shots 'cf-reqa-03-versions.png'))

    $numeric = @($ids | Where-Object { $_ -match '^version-row-\d+$' })
    Check 'every version row id is a CurseForge file id (numeric)' ($ids.Count -gt 0 -and $numeric.Count -eq $ids.Count) "numeric=$($numeric.Count) of $($ids.Count)"

    $cancel = Find-ById 'version-picker-cancel' -Optional
    if ($cancel) { [void](Invoke-Control $cancel); Start-Sleep -Milliseconds 800 }
    $cancel2 = Find-ById 'install-confirm-cancel' -Optional
    if ($cancel2) { [void](Invoke-Control $cancel2); Start-Sleep -Milliseconds 800 }
}
finally { Stop-App $proc }
End-Log
