$ErrorActionPreference = 'Stop'
. 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\gui-qa\env.ps1'

Start-Log $Shots 's11-settings-theme'
$proc = Start-App 's11'
try {
    Start-Sleep -Seconds 4

    # ── 11 CurseForge card ─────────────────────────────────────────────────
    Say '--- 11 설정 CurseForge card ---'
    Say ("  NEBULA_CURSEFORGE_API_KEY in the app's environment: " + $(if ($env:NEBULA_CURSEFORGE_API_KEY) { 'set' } else { 'unset' }))
    Send-Keys '^6'
    Start-Sleep -Seconds 2
    Check 'on 설정' ((Get-PageTitle) -eq '설정') (Get-PageTitle)
    $key = $null
    for ($i = 0; $i -lt 10 -and -not $key; $i++) {
        $key = Find-ById 'settings-curseforge-key' -Optional -TimeoutSeconds 1
        if (-not $key) { Send-Keys '{PGDN}' -NoClick; Start-Sleep -Milliseconds 250 }
    }
    Check 'settings-curseforge-key exists' ($null -ne $key)
    if ($key) {
        Say ("  field: name='$($key.Current.Name)' value=[" + (Get-Value $key) + "]")
        Say ("  card texts: " + ((@(Get-Texts) | Where-Object { $_ -match 'CurseForge|console' }) -join ' | '))
        [void](Save-WindowShot (Join-Path $Shots 'mods-11-settings-curseforge.png'))

        # Save with the key still empty.
        Type-Text $key ''
        Start-Sleep -Milliseconds 600
        $save = Find-ById 'settings-save' -Optional -TimeoutSeconds 3
        if (-not $save) {
            for ($i = 0; $i -lt 8 -and -not $save; $i++) { Send-Keys '{PGDN}' -NoClick; Start-Sleep -Milliseconds 250; $save = Find-ById 'settings-save' -Optional -TimeoutSeconds 1 }
        }
        if ($save) {
            Say ("  save: '$($save.Current.Name)' enabled=$($save.Current.IsEnabled)")
            [void](Invoke-Control $save)
            Start-Sleep -Seconds 3
            Say ("  toast: " + ((@(Get-Texts) | Where-Object { $_ -like '*저장*' }) -join ' | '))
        }
    }

    Say '--- 11 the CurseForge provider segment after saving an empty key ---'
    Send-Keys '^3' -NoClick
    Start-Sleep -Seconds 3
    [void](Invoke-Control (Find-ById 'mods-view-1'))
    Start-Sleep -Seconds 1
    $cf = Find-ById 'mods-provider-1'
    $mr = Find-ById 'mods-provider-0'
    Say ("  segment: Modrinth enabled=$($mr.Current.IsEnabled)  CurseForge enabled=$($cf.Current.IsEnabled)")
    Check 'the helper says where to put the key' (Has-Text '설정에서 CurseForge API 키를 입력하세요')
    [void](Save-WindowShot (Join-Path $Shots 'mods-11-provider-refused.png'))
    $before = Save-WindowShot (Join-Path $Shots 'mods-11-provider-a.png')
    [void](Invoke-Control $cf)
    Start-Sleep -Seconds 2
    $after = Save-WindowShot (Join-Path $Shots 'mods-11-provider-b.png')
    $r = Get-Rect $cf
    Say ("  segment pixels after clicking CurseForge: " + (Compare-Region $before $after $r.Left $r.Top $r.W $r.H))
    Type-Text (Find-ById 'mods-search') 'lithium'
    Send-Keys '{ENTER}'
    Start-Sleep -Seconds 5
    $rows = @(Find-ByIdPrefix 'mods-install-')
    Say ("  result ids after the refused switch: " + (($rows | ForEach-Object { $_.Current.AutomationId }) -join ', '))
    Check 'the search stayed on Modrinth' ((@($rows | Where-Object { $_.Current.AutomationId -like '*-modrinth-*' })).Count -eq $rows.Count) (($rows | ForEach-Object { $_.Current.AutomationId }) -join ', ')
    Check 'CurseForge is exposed to UIA as disabled' (-not $cf.Current.IsEnabled) "IsEnabled=$($cf.Current.IsEnabled)"
    [void](Save-WindowShot (Join-Path $Shots 'mods-11-provider-after-click.png'))

    # ── 12 theme toggle on the 모드 page ───────────────────────────────────
    Say '--- 12 theme toggle ---'
    Send-Keys '^3' -NoClick
    Start-Sleep -Seconds 2
    [void](Invoke-Control (Find-ById 'mods-view-0'))
    Start-Sleep -Seconds 3
    $tog = Find-ById 'theme-toggle'
    Say ("  toggle: '$($tog.Current.Name)'")
    $a = Save-WindowShot (Join-Path $Shots 'mods-12-theme-before.png')
    [void](Invoke-Control $tog)
    Start-Sleep -Seconds 2
    $b = Save-WindowShot (Join-Path $Shots 'mods-12-theme-after.png')
    $tog2 = Find-ById 'theme-toggle'
    Say ("  toggle after: '$($tog2.Current.Name)'   whole-window diff: " + (Compare-Shots $a $b))
    Check 'the theme toggle repaints the 모드 page' ((Compare-Shots $a $b) -gt 5) ("diff=" + (Compare-Shots $a $b))
    Check 'the toggle label flipped' ($tog2.Current.Name -ne $tog.Current.Name) "$($tog.Current.Name) -> $($tog2.Current.Name)"

    # back to the theme this session started in
    [void](Invoke-Control (Find-ById 'theme-toggle'))
    Start-Sleep -Seconds 2

    # ── 13 (setup): leave QAneo selected for the restart check ────────────
    Say '--- 13 setup: select the second server ---'
    [void](Invoke-Control (Find-ById 'mods-server-1'))
    Start-Sleep -Seconds 4
    Say ("  scope: " + ((Find-TextLike ' · Minecraft ') -join ' | '))
    Say ("  selected: " + ((Find-ByIdPrefix 'mods-server-' | ForEach-Object { "$($_.Current.AutomationId)=$(Get-Selected $_)" }) -join ' '))
}
finally { Stop-App $proc }
End-Log
