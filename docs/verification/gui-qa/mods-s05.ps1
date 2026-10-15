$ErrorActionPreference = 'Stop'
. 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\gui-qa\env.ps1'

Start-Log $Shots 's05-install'
$proc = Start-App 's05'
try {
    Start-Sleep -Seconds 4
    Send-Keys '^3'
    Start-Sleep -Seconds 3
    Say ("scope: " + ((Find-TextLike ' · Minecraft ') -join ' | '))
    [void](Invoke-Control (Find-ById 'mods-view-1'))
    Start-Sleep -Milliseconds 800
    Type-Text (Find-ById 'mods-search') 'sodium'
    Send-Keys '{ENTER}'
    Start-Sleep -Seconds 5

    # ── 5 install-confirm ──────────────────────────────────────────────────
    Say '--- 5 설치 on Sodium ---'
    $btn = Find-ById 'mods-install-modrinth-AANobbMI'
    Say ("  button: '$($btn.Current.Name)'")
    [void](Invoke-Control $btn)
    Check 'install-confirm opened' (Wait-Until { $null -ne (Find-ById 'install-confirm-submit' -Optional -TimeoutSeconds 1) } 40 'dialog')
    Start-Sleep -Seconds 1

    Say '  --- dialog texts ---'
    foreach ($t in (Get-Texts)) { Say ("    " + $t) }
    Say ("  ids: " + ((Get-Ids | Where-Object { $_ -like 'install-confirm*' }) -join ' ; '))
    Say ("  initial focus: " + (Focused-Desc))
    $submit = Find-ById 'install-confirm-submit'
    Say ("  primary: '$($submit.Current.Name)' enabled=$($submit.Current.IsEnabled)")
    Check 'the primary has the initial focus' ((Focused-Id) -eq 'install-confirm-submit') (Focused-Id)
    Check 'the dialog title names the server' (Has-Text 'QAfab-1.20.1에 설치')

    $deps = @(Find-ByIdPrefix 'install-confirm-dep-')
    Say ("  dependency rows: " + (($deps | ForEach-Object { "$($_.Current.AutomationId)='$($_.Current.Name)' role=$($_.Current.ControlType.ProgrammaticName.Replace('ControlType.','')) toggle=$(Get-Checked $_) patterns=[$(Get-Patterns $_)] enabled=$($_.Current.IsEnabled)" }) -join ' ; '))
    Check 'fabric-api is listed as a dependency' (($deps | Where-Object { $_.Current.Name -like '*abric*' }).Count -gt 0) (($deps | ForEach-Object { $_.Current.Name }) -join ',')
    Say ("  footer: " + ((Find-TextLike '개 · ') -join ' | '))
    [void](Save-WindowShot (Join-Path $Shots 'mods-05-install-confirm.png'))

    # ── 10 busy model, measured while the job runs ─────────────────────────
    Say '--- 10 busy model ---'
    $t0 = Get-Date
    [void](Invoke-Control $submit)
    Start-Sleep -Milliseconds 400
    $stillOpen1 = $null -ne (Find-ById 'install-confirm-submit' -Optional -TimeoutSeconds 1)
    Say ("  dialog open right after submit: $stillOpen1  (+{0:N1}s)" -f ((Get-Date) - $t0).TotalSeconds)
    if ($stillOpen1) {
        $s = Find-ById 'install-confirm-submit'
        $c = Find-ById 'install-confirm-cancel' -Optional -TimeoutSeconds 1
        Say ("  submit enabled=$($s.Current.IsEnabled)  cancel enabled=$(if ($c) { $c.Current.IsEnabled } else { '<gone>' })")
        Check 'the dialog controls are disabled while the job runs' (-not $s.Current.IsEnabled) "submit enabled=$($s.Current.IsEnabled)"
        [void](Save-WindowShot (Join-Path $Shots 'mods-10-busy-dialog.png'))

        Send-Keys '{ESC}' -NoClick
        Start-Sleep -Milliseconds 400
        $afterEsc = $null -ne (Find-ById 'install-confirm-submit' -Optional -TimeoutSeconds 1)
        Say ("  dialog open after Esc: $afterEsc")
        Check 'Esc is refused while busy' $afterEsc

        # the scrim: anywhere above the dialog card
        $r = Get-WindowRects
        [void][NebulaWin32]::SetCursorPos($r.Left + 400, $r.Top + 140)
        Start-Sleep -Milliseconds 60
        [NebulaWin32]::mouse_event([NebulaWin32]::MOUSEEVENTF_LEFTDOWN, 0, 0, 0, [IntPtr]::Zero)
        Start-Sleep -Milliseconds 50
        [NebulaWin32]::mouse_event([NebulaWin32]::MOUSEEVENTF_LEFTUP, 0, 0, 0, [IntPtr]::Zero)
        Start-Sleep -Milliseconds 500
        $afterScrim = $null -ne (Find-ById 'install-confirm-submit' -Optional -TimeoutSeconds 1)
        Say ("  dialog open after a scrim click: $afterScrim")
        Check 'the scrim does not dismiss a busy dialog' $afterScrim

        Send-Keys '^1' -NoClick
        Start-Sleep -Milliseconds 600
        $title = Get-PageTitle
        Say ("  page title after Ctrl+1: $title")
        Check 'navigation still works while busy' ($title -eq '빌드') "title=$title"
        [void](Save-WindowShot (Join-Path $Shots 'mods-10-busy-nav.png'))
        $gen = Find-ById 'build-generate' -Optional -TimeoutSeconds 2
        if (-not $gen) { $gen = Find-ByPartialName '배포본 생성' 'Button' -Optional -TimeoutSeconds 2 }
        if ($gen) {
            Say ("  빌드 primary: '$($gen.Current.Name)' enabled=$($gen.Current.IsEnabled)")
            Check 'the 빌드 primary is disabled while a mod job runs' (-not $gen.Current.IsEnabled)
        } else { Say '  !! no 빌드 primary found' }
        Say ("  top-bar pill: " + ((Find-TextLike '모드 설치 중') -join ' | '))
        Check 'the top-bar pill names the job' ((@(Find-TextLike '모드 설치 중')).Count -gt 0)

        $pr = Find-ById 'mods-search-progress' -Optional -TimeoutSeconds 1
        Say ("  ProgressRow(찾기) while off-page: " + ($null -ne $pr))
        Send-Keys '^3' -NoClick
        Start-Sleep -Milliseconds 600
        $pr = Find-ById 'mods-search-progress' -Optional -TimeoutSeconds 2
        if ($pr) {
            Say ("  ProgressRow: name='$($pr.Current.Name)' role=$($pr.Current.ControlType.ProgrammaticName) live=$(Get-LiveSetting $pr)")
            Check 'the ProgressRow announces politely (LiveSetting=Polite)' ((Get-LiveSetting $pr) -eq 'Polite') ("live=" + (Get-LiveSetting $pr))
            [void](Save-WindowShot (Join-Path $Shots 'mods-05-progress.png'))
        } else { Say '  (progress row already gone)' }
    } else {
        Say '  !! the install finished before the first probe; busy state not observable'
    }

    # ── back to the result ─────────────────────────────────────────────────
    [void](Wait-Until { $null -eq (Find-ById 'install-confirm-submit' -Optional -TimeoutSeconds 1) } 240 'job end')
    Say ("  job took {0:N1}s" -f ((Get-Date) - $t0).TotalSeconds)
    Send-Keys '^3' -NoClick
    Start-Sleep -Seconds 2
    Say ("  toast: " + ((Find-TextLike '설치했습니다') -join ' | '))
    Check 'the success toast names the server' ((@(Find-TextLike 'QAfab-1.20.1: 모드')).Count -gt 0) ((Find-TextLike '설치했습니다') -join ' | ')
    [void](Save-WindowShot (Join-Path $Shots 'mods-05-installed-toast.png'))

    $sodium = Find-ById 'mods-install-modrinth-AANobbMI' -Optional -TimeoutSeconds 3
    if ($sodium) {
        Say ("  Sodium row button: '$($sodium.Current.Name)' enabled=$($sodium.Current.IsEnabled)")
        Check 'the row became a disabled 설치됨' ((-not $sodium.Current.IsEnabled) -and ($sodium.Current.Name -like '*설치됨*')) "name='$($sodium.Current.Name)' enabled=$($sodium.Current.IsEnabled)"
    } else { Say '  !! Sodium row not visible' }
    Check '찾기 stayed on screen (no auto-switch)' ($null -ne (Find-ById 'mods-search' -Optional -TimeoutSeconds 2))

    Say '--- 설치됨 after the install ---'
    [void](Invoke-Control (Find-ById 'mods-view-0'))
    Start-Sleep -Seconds 4
    foreach ($t in (Get-Texts)) { Say ("    " + $t) }
    [void](Save-WindowShot (Join-Path $Shots 'mods-05-installed-list.png'))
    Check 'both new mods carry the Modrinth subtitle' ((@(Find-TextLike '· Modrinth')).Count -ge 2) ((Find-TextLike '· Modrinth') -join ' | ')

    Say '  --- disk ---'
    $ns = Join-Path $RootD 'servers\QAfab-1.20.1\fabricmods'
    foreach ($d in 'required','optionalon','optionaloff') {
        Say ("  $d : " + ((Get-ChildItem (Join-Path $ns $d) -ErrorAction SilentlyContinue | ForEach-Object { $_.Name }) -join ', '))
    }
}
finally { Stop-App $proc }
End-Log
