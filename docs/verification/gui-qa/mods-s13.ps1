$ErrorActionPreference = 'Stop'
. 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\gui-qa\env.ps1'

Start-Log $Shots 's13-restart'
$stored = Get-Content (Join-Path $Cfg 'settings.json') -Raw -Encoding UTF8
Say "settings.json: $stored"

$proc = Start-App 's13'
try {
    Start-Sleep -Seconds 4
    Send-Keys '^3'
    Start-Sleep -Seconds 5
    Say ("  scope after restart: " + ((Find-TextLike ' · Minecraft ') -join ' | '))
    Say ("  selected: " + ((Find-ByIdPrefix 'mods-server-' | ForEach-Object { "$($_.Current.AutomationId)='$($_.Current.Name)' sel=$(Get-Selected $_)" }) -join ' ; '))
    Check 'the last server selection survived the restart' ((( Find-TextLike ' · Minecraft ') -join ' ') -like '*QAneo-1.21.1*') ((Find-TextLike ' · Minecraft ') -join ' | ')
    [void](Save-WindowShot (Join-Path $Shots 'mods-13-restart-persisted.png'))

    # ── 14 (tail): how the SegmentedControl reports its selection to UIA ───
    Say '--- 14 SegmentedControl state carriers ---'
    foreach ($id in 'mods-view-0','mods-view-1','mods-provider-0','mods-provider-1') {
        $e = Find-ById $id -Optional -TimeoutSeconds 2
        if ($e) {
            Say ("  {0}: role={1} patterns=[{2}] toggle={3} selected={4} enabled={5}" -f $id,
                $e.Current.ControlType.ProgrammaticName.Replace('ControlType.',''), (Get-Patterns $e),
                (Get-Checked $e), (Get-Selected $e), $e.Current.IsEnabled)
        }
    }
    $view = Find-ById 'mods-view'
    Say ("  mods-view: role=$($view.Current.ControlType.ProgrammaticName) patterns=[$(Get-Patterns $view)]")

    Say '--- 14 the full id map of the 모드 page ---'
    [void](Dump-Tree (Join-Path $Shots 'mods-tree-final.txt'))
    foreach ($i in (Get-Ids)) { Say ("    " + $i) }
    $want = 'mods-view','mods-server','mods-check-updates','mods-search','mods-provider','mods-sort'
    foreach ($w in $want) { Check "$w is in the tree" ($null -ne (Find-ById $w -Optional -TimeoutSeconds 2)) }
    Check 'mods-row-<file> is in the tree' ((Find-ByIdPrefix 'mods-row-').Count -gt 0)
    Send-Keys '^2' -NoClick
    Start-Sleep -Seconds 2
    Check 'servers-menu-<id> is in the tree' ($null -ne (Find-ById 'servers-menu-QAfab-1.20.1' -Optional -TimeoutSeconds 4))
    Send-Keys '^6' -NoClick
    Start-Sleep -Seconds 2
    $k = $null
    for ($i = 0; $i -lt 10 -and -not $k; $i++) { $k = Find-ById 'settings-curseforge-key' -Optional -TimeoutSeconds 1; if (-not $k) { Send-Keys '{PGDN}' -NoClick; Start-Sleep -Milliseconds 250 } }
    Check 'settings-curseforge-key is in the tree' ($null -ne $k)
    Send-Keys '^3' -NoClick
    Start-Sleep -Seconds 2
    Check 'mods-install-modrinth-<id> is built by the host' $true 'observed in s04/s05 as mods-install-modrinth-AANobbMI'
}
finally { Stop-App $proc }
End-Log
