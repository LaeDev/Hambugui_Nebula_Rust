$ErrorActionPreference = 'Stop'
. 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\gui-qa\env.ps1'

# s10d showed the dialog gone and the 빌드 primary enabled after Ctrl+1, but its
# poll could not say whether the job was still running at that instant. The
# top-bar pill is the page-independent busy carrier, so this run records it on
# every tick and fires Ctrl+1 from inside the loop.
$pad = Join-Path $RootD 'servers\QAneo-1.21.1\files\pad.bin'
Start-Log $Shots 's10e-busy-nav'
if (-not (Test-Path $pad)) {
    Say 'writing a 2 GB pad file'
    $fs = [System.IO.File]::Create($pad)
    $buf = New-Object byte[] (16MB)
    (New-Object Random 7).NextBytes($buf)
    for ($k = 0; $k -lt 128; $k++) { $fs.Write($buf, 0, $buf.Length) }
    $fs.Close()
}

$proc = Start-App 's10e'
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
    [void](Activate-Win)

    $t0 = Get-Date
    [void](Invoke-Control (Find-ById 'server-duplicate-submit'))
    $navAt = $null
    $rows = New-Object System.Collections.Generic.List[object]
    for ($i = 0; $i -lt 300; $i++) {
        $pill = @(Get-Texts) | Where-Object { $_ -like '*복제 중*' } | Select-Object -First 1
        $dlg = $null -ne (Find-ById 'server-duplicate-submit' -Optional -TimeoutSeconds 0)
        $gen = Find-ById 'build-generate' -Optional -TimeoutSeconds 0
        $row = [pscustomobject]@{
            T = ((Get-Date) - $t0).TotalSeconds
            Pill = [bool]$pill
            PillText = $pill
            Dialog = $dlg
            BuildPrimary = $(if ($gen) { $gen.Current.IsEnabled } else { $null })
        }
        $rows.Add($row)
        Say ("  [{0,5:N1}s] pill={1} '{2}' dialog={3} build-generate-enabled={4}" -f `
            $row.T, $row.Pill, $row.PillText, $row.Dialog, $(if ($null -ne $row.BuildPrimary) { $row.BuildPrimary } else { 'n/a' }))
        if ($i -eq 2 -and -not $navAt) {
            [System.Windows.Forms.SendKeys]::SendWait('^1')
            $navAt = $row.T
            Say ("  >>> Ctrl+1 sent at {0:N1}s" -f $navAt)
        }
        if (-not $row.Pill -and $i -gt 4) { break }
        Start-Sleep -Milliseconds 100
    }

    $busyRows = @($rows | Where-Object { $_.Pill })
    $lastBusy = ($busyRows | Select-Object -Last 1)
    Say ("  pill last seen at {0:N1}s" -f $lastBusy.T)
    $afterNavBusy = @($busyRows | Where-Object { $_.T -gt $navAt })
    Say ("  ticks that were still busy AFTER Ctrl+1: " + $afterNavBusy.Count)
    foreach ($r in $afterNavBusy) {
        Say ("    [{0,5:N1}s] dialog={1} build-generate-enabled={2}" -f $r.T, $r.Dialog, $(if ($null -ne $r.BuildPrimary) { $r.BuildPrimary } else { 'n/a' }))
    }
    Check 'the probe caught ticks that were still busy after Ctrl+1' ($afterNavBusy.Count -gt 0)
    if ($afterNavBusy.Count -gt 0) {
        Check 'the open dialog survives Ctrl+1 while the job runs (§8.6)' (@($afterNavBusy | Where-Object { $_.Dialog }).Count -eq $afterNavBusy.Count) `
            ("dialog open on " + @($afterNavBusy | Where-Object { $_.Dialog }).Count + " of " + $afterNavBusy.Count + " busy ticks")
        $seen = @($afterNavBusy | Where-Object { $null -ne $_.BuildPrimary })
        Check 'the 빌드 primary is disabled while a job runs (§8.6)' (($seen.Count -gt 0) -and (@($seen | Where-Object { -not $_.BuildPrimary }).Count -eq $seen.Count)) `
            ("enabled on " + @($seen | Where-Object { $_.BuildPrimary }).Count + " of " + $seen.Count + " busy ticks on 빌드")
    }
    [void](Save-WindowShot (Join-Path $Shots 'mods-10-busy-nav.png'))
}
finally {
    Stop-App $proc
    Remove-Item -Recurse -Force (Join-Path $RootD 'servers\QAbusy-1.21.1') -ErrorAction SilentlyContinue
    Get-ChildItem (Join-Path $RootD 'servers') -Force -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -like '.*partial*' } | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -Force $pad -ErrorAction SilentlyContinue
    Say 'cleaned up'
}
End-Log
