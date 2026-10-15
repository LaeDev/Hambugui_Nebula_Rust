param([switch] $KeepOpen)
$ErrorActionPreference = 'Stop'
. 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\gui-qa\env.ps1'

Start-Log $Shots 's01-launch'
$proc = Start-App 's01'
try {
    Start-Sleep -Seconds 4

    # ── 1. launch ──────────────────────────────────────────────────────────
    Say '--- 1 launch ---'
    Say ("title: " + (Get-PageTitle))
    [void](Save-WindowShot (Join-Path $Shots 'mods-01-launch.png'))

    Send-Keys '^3'
    Start-Sleep -Milliseconds 900
    $t = Get-PageTitle
    Check 'Ctrl+3 reaches the 모드 page' ($t -eq '모드') "title=$t"
    [void](Save-WindowShot (Join-Path $Shots 'mods-01-ctrl3.png'))

    # ── PillStrip shape ────────────────────────────────────────────────────
    Say '--- pill strip ---'
    $strip = Find-ById 'mods-server' -Optional
    Check 'mods-server exists' ($null -ne $strip)
    if ($strip) {
        Say ("  role: " + $strip.Current.ControlType.ProgrammaticName)
        Say ("  name: " + $strip.Current.Name)
    }
    $pills = @(Find-ByIdPrefix 'mods-server-')
    Say ("  pills: " + (($pills | ForEach-Object { "$($_.Current.AutomationId)='$($_.Current.Name)' role=$($_.Current.ControlType.ProgrammaticName.Replace('ControlType.','')) checked=$(Get-Checked $_)" }) -join ' ; '))
    Check 'two pills, one per server' ($pills.Count -eq 2) "count=$($pills.Count)"

    # ── one tab stop ───────────────────────────────────────────────────────
    Say '--- tab order on 모드 ---'
    Activate-Win | Out-Null
    Send-Keys '^{HOME}' -NoClick
    Start-Sleep -Milliseconds 300
    $stops = @()
    for ($i = 0; $i -lt 16; $i++) {
        Send-Keys '{TAB}' -NoClick
        Start-Sleep -Milliseconds 180
        $d = Focused-Desc
        $stops += $d
        Say ("  {0,2}: {1}" -f $i, $d)
    }
    $stripStops = @($stops | Where-Object { $_ -match 'mods-server' })
    Say ("  stops mentioning mods-server: " + $stripStops.Count)

    # ── roving focus vs selection ──────────────────────────────────────────
    Say '--- Right/Left move the ring without selecting ---'
    # Focus the strip by clicking the selected pill (a click also selects it,
    # which is a no-op when it is already selected).
    $p0 = Find-ById 'mods-server-0'
    Click-Control $p0
    Start-Sleep -Milliseconds 700
    $before = @(Find-ByIdPrefix 'mods-server-' | ForEach-Object { "$($_.Current.AutomationId)=$(Get-Checked $_)" })
    Say ("  checked before: " + ($before -join ' '))
    $shotA = Save-WindowShot (Join-Path $Shots 'mods-01-pill-ring-a.png')

    Send-Keys '{RIGHT}' -NoClick
    Start-Sleep -Milliseconds 500
    $after = @(Find-ByIdPrefix 'mods-server-' | ForEach-Object { "$($_.Current.AutomationId)=$(Get-Checked $_)" })
    Say ("  checked after Right: " + ($after -join ' '))
    Check 'Right does not change the selection' (($before -join ' ') -eq ($after -join ' ')) "$($before -join ' ') -> $($after -join ' ')"
    Say ("  UIA focus after Right: " + (Focused-Desc))
    $shotB = Save-WindowShot (Join-Path $Shots 'mods-01-pill-ring-b.png')

    $r0 = Get-Rect (Find-ById 'mods-server-0')
    $r1 = Get-Rect (Find-ById 'mods-server-1')
    $d0 = Compare-Region $shotA $shotB $r0.Left $r0.Top $r0.W $r0.H
    $d1 = Compare-Region $shotA $shotB $r1.Left $r1.Top $r1.W $r1.H
    Say ("  ring pixels: pill0 diff=$d0  pill1 diff=$d1")
    Check 'the ring visibly moved off pill 0 onto pill 1' (($d0 -gt 0.2) -and ($d1 -gt 0.2)) "d0=$d0 d1=$d1"

    Send-Keys '{LEFT}' -NoClick
    Start-Sleep -Milliseconds 500
    $afterL = @(Find-ByIdPrefix 'mods-server-' | ForEach-Object { "$($_.Current.AutomationId)=$(Get-Checked $_)" })
    Check 'Left does not change the selection either' (($before -join ' ') -eq ($afterL -join ' ')) ($afterL -join ' ')

    # ── Enter selects ──────────────────────────────────────────────────────
    Say '--- Enter selects the focused pill ---'
    $scopeBefore = (Find-TextLike ' · Minecraft ') -join ' | '
    Say ("  scope before: " + $scopeBefore)
    Send-Keys '{RIGHT}' -NoClick
    Start-Sleep -Milliseconds 300
    Send-Keys '{ENTER}' -NoClick
    Start-Sleep -Seconds 3
    $sel = @(Find-ByIdPrefix 'mods-server-' | ForEach-Object { "$($_.Current.AutomationId)=$(Get-Checked $_)" })
    Say ("  checked after Enter: " + ($sel -join ' '))
    $scopeAfter = (Find-TextLike ' · Minecraft ') -join ' | '
    Say ("  scope after: " + $scopeAfter)
    Check 'Enter moved the selection to pill 1' (($sel -join ' ') -like '*mods-server-1=On*') ($sel -join ' ')
    Check 'the scope line followed the server' ($scopeAfter -ne $scopeBefore) "$scopeBefore -> $scopeAfter"
    [void](Save-WindowShot (Join-Path $Shots 'mods-01-pillstrip.png'))

    # back to the Fabric server for the scenarios that follow
    Send-Keys '{LEFT}' -NoClick
    Start-Sleep -Milliseconds 300
    Send-Keys '{ENTER}' -NoClick
    Start-Sleep -Seconds 3
    Say ("  back to: " + ((Find-TextLike ' · Minecraft ') -join ' | '))

    # ── 14. accessibility: the whole id map of the 모드 page ────────────────
    Say '--- 14 AutomationIds on 모드 ---'
    [void](Dump-Tree (Join-Path $Shots 'mods-tree-modspage.txt'))
    $ids = @(Get-Ids)
    Say ("  ids: " + $ids.Count)
    foreach ($i in $ids) { Say ("    " + $i) }
}
finally {
    if (-not $KeepOpen) { Stop-App $proc }
}
End-Log
