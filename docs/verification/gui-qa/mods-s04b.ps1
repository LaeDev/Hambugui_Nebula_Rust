$ErrorActionPreference = 'Stop'
. 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\gui-qa\env.ps1'

# s04 found the icons already on disk, so its before/after shot could not show
# them arriving. Empty the cache first and shoot the first seconds.
Remove-Item -Recurse -Force (Join-Path $RootD 'cache\icons') -ErrorAction SilentlyContinue

Start-Log $Shots 's04b-icons-more'
$proc = Start-App 's04b'
try {
    Start-Sleep -Seconds 4
    Send-Keys '^3'
    Start-Sleep -Seconds 3
    [void](Invoke-Control (Find-ById 'mods-view-1'))
    Start-Sleep -Seconds 1

    Say '--- 4 icons arrive after the rows ---'
    Type-Text (Find-ById 'mods-search') 'sodium'
    Activate-Win | Out-Null
    [System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
    $Cap = New-Object System.Collections.Generic.List[string]
    for ($i = 0; $i -lt 8; $i++) {
        $Cap.Add((Save-WindowShot (Join-Path $Shots ("mods-04b-frame-{0}.png" -f $i))))
        Start-Sleep -Milliseconds 500
    }
    for ($i = 1; $i -lt $Cap.Count; $i++) {
        Say ("  frame 0 -> {0}: diff {1}" -f $i, (Compare-Shots $Cap[0] $Cap[$i]))
    }
    $cache = @(Get-ChildItem (Join-Path $RootD 'cache\icons') -Filter *.png -ErrorAction SilentlyContinue)
    Say ("  icon cache: $($cache.Count) png")
    Check 'icons were fetched and cached' ($cache.Count -gt 0) "$($cache.Count) png"
    Check 'the rows visibly changed while the icons landed' ((Compare-Shots $Cap[0] $Cap[$Cap.Count - 1]) -gt 0.1)
    Copy-Item $Cap[0] (Join-Path $Shots 'mods-04-icons-before.png') -Force
    Copy-Item $Cap[$Cap.Count - 1] (Join-Path $Shots 'mods-04-icons-after.png') -Force
    foreach ($f in $Cap) { Remove-Item $f -Force }

    # ── 더 보기 lives below the last hit; the page ScrollView owns it ────────
    Say '--- 4 더 보기 ---'
    $before = (Find-ByIdPrefix 'mods-install-').Count
    Say ("  visible rows before: $before")
    $more = $null
    for ($i = 0; $i -lt 14 -and -not $more; $i++) {
        Send-Keys '{PGDN}' -NoClick
        Start-Sleep -Milliseconds 250
        $more = Find-ById 'mods-search-more' -Optional -TimeoutSeconds 1
    }
    Check '더 보기 is reachable by PageDown' ($null -ne $more)
    if ($more) {
        Say ("  button: '$($more.Current.Name)'")
        $ids0 = @(Find-ByIdPrefix 'mods-install-' | ForEach-Object { $_.Current.AutomationId })
        [void](Save-WindowShot (Join-Path $Shots 'mods-04-more-before.png'))
        [void](Invoke-Control $more)
        Start-Sleep -Seconds 4
        $ids1 = @(Find-ByIdPrefix 'mods-install-' | ForEach-Object { $_.Current.AutomationId })
        Say ("  visible ids before: " + ($ids0 -join ','))
        Say ("  visible ids after : " + ($ids1 -join ','))
        $new = @($ids1 | Where-Object { $ids0 -notcontains $_ })
        Check '더 보기 appended a second page (kept the old rows, added new ones)' ($new.Count -gt 0) ("new=" + ($new -join ','))
        [void](Save-WindowShot (Join-Path $Shots 'mods-04-more-after.png'))
        Say ("  더 보기 still present: " + ($null -ne (Find-ById 'mods-search-more' -Optional -TimeoutSeconds 2)))
    }
}
finally { Stop-App $proc }
End-Log
