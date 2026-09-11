$ErrorActionPreference = 'Stop'
. 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\gui-qa\env.ps1'

# Bytes are not what makes a copy slow on this machine (2 GB went in ~1 s).
# Per-file overhead is: 15,000 tiny files take seconds to walk and recreate,
# which finally gives §8.6 a job long enough to navigate during.
$padDir = Join-Path $RootD 'servers\QAneo-1.21.1\files\pad'
Start-Log $Shots 's10f-busy-nav'
if (-not (Test-Path $padDir)) {
    Say 'writing 15,000 small pad files'
    New-Item -ItemType Directory -Force -Path $padDir | Out-Null
    $bytes = New-Object byte[] 1024
    for ($k = 0; $k -lt 15000; $k++) {
        [System.IO.File]::WriteAllBytes((Join-Path $padDir ("p{0:D5}.bin" -f $k)), $bytes)
    }
}
Say ("pad files: " + (Get-ChildItem $padDir).Count)

$proc = Start-App 's10f'
try {
    Start-Sleep -Seconds 4
    Send-Keys '^2'
    Start-Sleep -Seconds 5
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
    $escAt = $null
    $rows = New-Object System.Collections.Generic.List[object]
    for ($i = 0; $i -lt 400; $i++) {
        $pill = @(Get-Texts) | Where-Object { $_ -like '*복제 중*' } | Select-Object -First 1
        $dlg = $null -ne (Find-ById 'server-duplicate-submit' -Optional -TimeoutSeconds 0)
        $gen = Find-ById 'build-generate' -Optional -TimeoutSeconds 0
        $row = [pscustomobject]@{
            T = ((Get-Date) - $t0).TotalSeconds
            Pill = [bool]$pill; PillText = $pill; Dialog = $dlg
            BuildPrimary = $(if ($gen) { $gen.Current.IsEnabled } else { $null })
        }
        $rows.Add($row)
        Say ("  [{0,5:N1}s] pill={1} '{2}' dialog={3} build-generate-enabled={4}" -f `
            $row.T, $row.Pill, $row.PillText, $row.Dialog, $(if ($null -ne $row.BuildPrimary) { $row.BuildPrimary } else { 'n/a' }))
        if ($row.Pill -and -not $escAt) {
            [System.Windows.Forms.SendKeys]::SendWait('{ESC}')
            $escAt = $row.T
            Say ("  >>> Esc sent at {0:N1}s" -f $escAt)
        } elseif ($escAt -and -not $navAt -and $row.T -gt ($escAt + 0.5)) {
            [System.Windows.Forms.SendKeys]::SendWait('^1')
            $navAt = $row.T
            Say ("  >>> Ctrl+1 sent at {0:N1}s" -f $navAt)
        }
        if (-not $row.Pill -and $i -gt 3) { break }
        Start-Sleep -Milliseconds 150
    }
    $busy = @($rows | Where-Object { $_.Pill })
    Say ("  pill live from {0:N1}s to {1:N1}s ({2} ticks)" -f $busy[0].T, $busy[-1].T, $busy.Count)

    $afterEsc = @($busy | Where-Object { $escAt -and $_.T -gt $escAt })
    Check 'Esc is refused while busy' (($afterEsc.Count -gt 0) -and (@($afterEsc | Where-Object { $_.Dialog }).Count -eq $afterEsc.Count)) `
        ("dialog open on " + @($afterEsc | Where-Object { $_.Dialog }).Count + "/" + $afterEsc.Count + " busy ticks after Esc")

    $afterNav = @($busy | Where-Object { $navAt -and $_.T -gt $navAt })
    Check 'the probe caught busy ticks after Ctrl+1' ($afterNav.Count -gt 0) ("$($afterNav.Count) ticks")
    if ($afterNav.Count -gt 0) {
        foreach ($r in $afterNav) {
            Say ("    after nav [{0,5:N1}s] dialog={1} build-generate-enabled={2}" -f $r.T, $r.Dialog, $(if ($null -ne $r.BuildPrimary) { $r.BuildPrimary } else { 'n/a' }))
        }
        Check 'the open dialog survives Ctrl+1 while the job runs (§8.6)' (@($afterNav | Where-Object { $_.Dialog }).Count -eq $afterNav.Count) `
            ("open on " + @($afterNav | Where-Object { $_.Dialog }).Count + "/" + $afterNav.Count)
        $onBuild = @($afterNav | Where-Object { $null -ne $_.BuildPrimary })
        Check 'navigation reached 빌드 while the job runs' ($onBuild.Count -gt 0) ("$($onBuild.Count) ticks saw build-generate")
        if ($onBuild.Count -gt 0) {
            Check 'the 빌드 primary is disabled while a job runs (§8.6)' (@($onBuild | Where-Object { -not $_.BuildPrimary }).Count -eq $onBuild.Count) `
                ("enabled on " + @($onBuild | Where-Object { $_.BuildPrimary }).Count + "/" + $onBuild.Count)
        }
    }
    [void](Save-WindowShot (Join-Path $Shots 'mods-10-busy-nav.png'))
    $toast = @()
    for ($i = 0; $i -lt 40 -and $toast.Count -eq 0; $i++) {
        $toast = @(Find-TextLike '복제했습니다') + @(Find-TextLike '복제 실패')
        if ($toast.Count -eq 0) { Start-Sleep -Milliseconds 250 }
    }
    Say ("  toast: " + (($toast | Select-Object -Unique) -join ' | '))
}
finally {
    Stop-App $proc
    Remove-Item -Recurse -Force (Join-Path $RootD 'servers\QAbusy-1.21.1') -ErrorAction SilentlyContinue
    Get-ChildItem (Join-Path $RootD 'servers') -Force -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -like '.*partial*' } | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force $padDir -ErrorAction SilentlyContinue
    Say 'cleaned up'
}
End-Log
