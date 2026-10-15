$ErrorActionPreference = 'Stop'
. 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\gui-qa\env.ps1'

# Sodium 0.5.13 for 1.20.1 declares no required dependency, so s05's dialog had
# nothing to pre-check. Sodium Extra declares fabric-api (not installed → a
# pre-checked switch) and Sodium (installed → a disabled 설치됨 row), which is
# the branch §8.4 describes.
Start-Log $Shots 's05b-install-deps'
$proc = Start-App 's05b'
try {
    Start-Sleep -Seconds 4
    Send-Keys '^3'
    Start-Sleep -Seconds 3
    [void](Invoke-Control (Find-ById 'mods-view-1'))
    Start-Sleep -Milliseconds 800
    Type-Text (Find-ById 'mods-search') 'sodium'
    Send-Keys '{ENTER}'
    Start-Sleep -Seconds 5

    Say '--- the already-installed row (§8.4) ---'
    $sod = Find-ById 'mods-install-modrinth-AANobbMI'
    Say ("  Sodium button: UIA Name='$($sod.Current.Name)' enabled=$($sod.Current.IsEnabled)")
    Say ("  visible labels near it: " + ((@(Get-Texts) | Where-Object { $_ -eq '설치됨' -or $_ -eq '설치' }) -join ', '))
    [void](Save-WindowShot (Join-Path $Shots 'mods-05-row-installed.png'))

    Say '--- 5 설치 on Sodium Extra (fabric-api dependency) ---'
    [void](Invoke-Control (Find-ById 'mods-install-modrinth-PtjYWJkn'))
    Check 'install-confirm opened' (Wait-Until { $null -ne (Find-ById 'install-confirm-submit' -Optional -TimeoutSeconds 1) } 40 'dialog')
    Start-Sleep -Seconds 1
    Say '  --- dialog texts ---'
    foreach ($t in (Get-Texts)) { Say ("    " + $t) }
    Say ("  ids: " + ((Get-Ids | Where-Object { $_ -like 'install-confirm*' }) -join ' ; '))
    Say ("  initial focus: " + (Focused-Desc))
    $deps = @(Find-ByIdPrefix 'install-confirm-dep-')
    Say ("  dependency rows: " + (($deps | ForEach-Object { "$($_.Current.AutomationId)='$($_.Current.Name)' role=$($_.Current.ControlType.ProgrammaticName.Replace('ControlType.','')) toggle=$(Get-Checked $_) enabled=$($_.Current.IsEnabled)" }) -join ' ; '))
    Check 'a fabric-api dependency row exists' (($deps | Where-Object { $_.Current.Name -match 'abric' }).Count -gt 0) (($deps | ForEach-Object { $_.Current.Name }) -join ',')
    $fa = $deps | Where-Object { $_.Current.Name -match 'abric' } | Select-Object -First 1
    if ($fa) { Check 'the required dependency is pre-checked' ((Get-Checked $fa) -eq 'On') ("toggle=" + (Get-Checked $fa)) }
    Check 'the installed dependency (Sodium) is listed disabled with 설치됨' (Has-Text '설치됨') ((@(Get-Texts) | Where-Object { $_ -like '*설치됨*' }) -join ' | ')
    $submit = Find-ById 'install-confirm-submit'
    Say ("  footer: " + ((Find-TextLike '개 · ') -join ' | ') + "   primary='$($submit.Current.Name)'")
    [void](Save-WindowShot (Join-Path $Shots 'mods-05-install-confirm.png'))

    # ── 10 busy model, probed from t=0 ────────────────────────────────────
    Say '--- 10 busy model ---'
    $t0 = Get-Date
    [void](Invoke-Control $submit)
    $seen = @{ dialog = $false; pill = $false; progress = $false; disabled = $false }
    $liveSetting = ''
    $progressText = @()
    for ($i = 0; $i -lt 60; $i++) {
        $d = Find-ById 'install-confirm-submit' -Optional -TimeoutSeconds 0
        if ($d) {
            $seen.dialog = $true
            if (-not $d.Current.IsEnabled) { $seen.disabled = $true }
        }
        if ((@(Find-TextLike '모드 설치 중')).Count -gt 0) { $seen.pill = $true }
        $pr = Find-ById 'mods-search-progress' -Optional -TimeoutSeconds 0
        if ($pr) {
            $seen.progress = $true
            $liveSetting = Get-LiveSetting $pr
            $progressText += $pr.Current.Name
        }
        if (-not $d -and $i -gt 4) { break }
        Start-Sleep -Milliseconds 120
    }
    Say ("  observed: dialog-stays-open=$($seen.dialog) controls-disabled=$($seen.disabled) top-bar-pill=$($seen.pill) ProgressRow=$($seen.progress)")
    Say ("  ProgressRow LiveSetting=$liveSetting")
    Say ("  ProgressRow text samples: " + (($progressText | Select-Object -Unique) -join ' | '))
    Check 'the dialog stays open while the job runs' $seen.dialog
    Check 'its controls are disabled' $seen.disabled
    Check 'the top-bar pill names the job' $seen.pill
    Check 'a ProgressRow appears in the 찾기 card' $seen.progress
    Check 'the ProgressRow is a polite live region' ($liveSetting -eq 'Polite') "live=$liveSetting"

    # toast, caught before it expires
    $toast = @()
    for ($i = 0; $i -lt 40 -and $toast.Count -eq 0; $i++) {
        $toast = @(Find-TextLike '설치했습니다')
        if ($toast.Count -eq 0) { Start-Sleep -Milliseconds 250 }
    }
    Say ("  toast: " + (($toast | Select-Object -Unique) -join ' | '))
    Check 'the success toast names the server' ((($toast | Select-Object -Unique) -join ' ') -like '*QAfab-1.20.1*') (($toast | Select-Object -Unique) -join ' | ')
    [void](Save-WindowShot (Join-Path $Shots 'mods-05-success-toast.png'))
    Say ("  job took {0:N1}s" -f ((Get-Date) - $t0).TotalSeconds)

    Start-Sleep -Seconds 2
    $se = Find-ById 'mods-install-modrinth-PtjYWJkn' -Optional -TimeoutSeconds 3
    if ($se) { Say ("  Sodium Extra button: Name='$($se.Current.Name)' enabled=$($se.Current.IsEnabled)") }
    Check 'the installed row is disabled' ($se -and -not $se.Current.IsEnabled)

    Say '--- 설치됨 after the install ---'
    [void](Invoke-Control (Find-ById 'mods-view-0'))
    Start-Sleep -Seconds 5
    foreach ($t in (Get-Texts)) { Say ("    " + $t) }
    [void](Save-WindowShot (Join-Path $Shots 'mods-05-installed-list.png'))
    $mod = @(Find-TextLike '· Modrinth')
    Check 'the Modrinth mods carry the Modrinth subtitle' ($mod.Count -ge 2) ($mod -join ' | ')

    $ns = Join-Path $RootD 'servers\QAfab-1.20.1\fabricmods'
    foreach ($d in 'required','optionalon','optionaloff') {
        Say ("  disk $d : " + ((Get-ChildItem (Join-Path $ns $d) -ErrorAction SilentlyContinue | ForEach-Object { $_.Name }) -join ', '))
    }
}
finally { Stop-App $proc }
End-Log
