$ErrorActionPreference = 'Stop'
. 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\gui-qa\env.ps1'

Start-Log $Shots 's09-server-menu'
$proc = Start-App 's09'
try {
    Start-Sleep -Seconds 4
    Send-Keys '^2'
    Start-Sleep -Seconds 4
    Check 'on 서버' ((Get-PageTitle) -eq '서버') (Get-PageTitle)
    Say ("  ids: " + ((Get-Ids | Where-Object { $_ -like 'server*' }) -join ' ; '))
    [void](Save-WindowShot (Join-Path $Shots 'mods-09-servers.png'))

    Say '--- 9 server-menu ---'
    $menu = Find-ById 'servers-menu-QAneo-1.21.1'
    Say ("  chevron: '$($menu.Current.Name)' role=$($menu.Current.ControlType.ProgrammaticName)")
    Check 'the row trailing control is the menu chevron' ($menu.Current.Name -like '*메뉴*') $menu.Current.Name
    [void](Invoke-Control $menu)
    Start-Sleep -Seconds 1
    Say ("  menu ids: " + ((Get-Ids | Where-Object { $_ -like 'server-menu*' }) -join ' ; '))
    foreach ($t in (Get-Texts)) { Say ("    " + $t) }
    Check 'the menu offers 모드 관리 / 폴더 열기 / 복제 / 삭제' (
        ($null -ne (Find-ById 'server-menu-mods' -Optional -TimeoutSeconds 2)) -and
        ($null -ne (Find-ById 'server-menu-open-folder' -Optional -TimeoutSeconds 2)) -and
        ($null -ne (Find-ById 'server-menu-duplicate' -Optional -TimeoutSeconds 2)) -and
        ($null -ne (Find-ById 'server-menu-delete' -Optional -TimeoutSeconds 2)))
    [void](Save-WindowShot (Join-Path $Shots 'mods-09-server-menu.png'))

    Say '--- 9 복제 ---'
    [void](Invoke-Control (Find-ById 'server-menu-duplicate'))
    Check 'server-duplicate opened' (Wait-Until { $null -ne (Find-ById 'server-duplicate-submit' -Optional -TimeoutSeconds 1) } 20 'dialog')
    Start-Sleep -Milliseconds 800
    foreach ($t in (Get-Texts)) { Say ("    " + $t) }
    Say ("  initial focus: " + (Focused-Desc))
    Check 'the field has the initial focus' ((Focused-Id) -eq 'server-duplicate-id') (Focused-Id)
    Type-Text (Find-ById 'server-duplicate-id') 'QAcopy'
    Start-Sleep -Milliseconds 500
    [void](Save-WindowShot (Join-Path $Shots 'mods-09-duplicate.png'))
    $t0 = Get-Date
    [void](Invoke-Control (Find-ById 'server-duplicate-submit'))
    $seenPill = $false
    for ($i = 0; $i -lt 200; $i++) {
        if ((@(Find-TextLike '서버 복제 중')).Count -gt 0) { $seenPill = $true }
        if ($i -gt 3 -and $null -eq (Find-ById 'server-duplicate-submit' -Optional -TimeoutSeconds 0)) { break }
        Start-Sleep -Milliseconds 150
    }
    $toast = @()
    for ($i = 0; $i -lt 60 -and $toast.Count -eq 0; $i++) {
        $toast = @(Find-TextLike '복제했습니다') + @(Find-TextLike '복제 실패')
        if ($toast.Count -eq 0) { Start-Sleep -Milliseconds 250 }
    }
    Say ("  pill=$seenPill toast=" + (($toast | Select-Object -Unique) -join ' | ') + ("  {0:N1}s" -f ((Get-Date) - $t0).TotalSeconds))
    Check 'the 복제 pill names the job' $seenPill
    Start-Sleep -Seconds 3
    Say ("  disk servers: " + ((Get-ChildItem (Join-Path $RootD 'servers') -Directory | ForEach-Object { $_.Name }) -join ', '))
    Check 'the copy is on disk' (Test-Path (Join-Path $RootD 'servers\QAcopy-1.21.1'))
    Check 'the copy has a row' ($null -ne (Find-ById 'servers-menu-QAcopy-1.21.1' -Optional -TimeoutSeconds 8))
    [void](Save-WindowShot (Join-Path $Shots 'mods-09-duplicated.png'))

    Say '--- 9 삭제 ---'
    [void](Invoke-Control (Find-ById 'servers-menu-QAcopy-1.21.1'))
    Start-Sleep -Milliseconds 800
    [void](Invoke-Control (Find-ById 'server-menu-delete'))
    Check 'server-delete opened' (Wait-Until { $null -ne (Find-ById 'server-delete-submit' -Optional -TimeoutSeconds 1) } 20 'dialog')
    Start-Sleep -Milliseconds 800
    foreach ($t in (Get-Texts)) { Say ("    " + $t) }
    Say ("  initial focus: " + (Focused-Desc))
    $sub = Find-ById 'server-delete-submit'
    Check '삭제 starts disabled' (-not $sub.Current.IsEnabled)
    [void](Save-WindowShot (Join-Path $Shots 'mods-09-delete-empty.png'))

    Type-Text (Find-ById 'server-delete-confirm') 'QAcopy'
    Start-Sleep -Milliseconds 500
    Check '삭제 stays disabled for a partial id' (-not (Find-ById 'server-delete-submit').Current.IsEnabled)

    Type-Text (Find-ById 'server-delete-confirm') 'QAcopy-1.21.1 '
    Start-Sleep -Milliseconds 600
    $trail = (Find-ById 'server-delete-submit').Current.IsEnabled
    Say ("  with a trailing space: 삭제 enabled=$trail  value=[" + (Get-Value (Find-ById 'server-delete-confirm')) + "]")
    Check '삭제 stays disabled with a trailing space' (-not $trail)
    [void](Save-WindowShot (Join-Path $Shots 'mods-09-delete-trailing-space.png'))

    Send-Keys '{ENTER}'
    Start-Sleep -Seconds 2
    Check 'Enter does nothing while 삭제 is disabled' ($null -ne (Find-ById 'server-delete-submit' -Optional -TimeoutSeconds 2))
    Check 'the server is still on disk' (Test-Path (Join-Path $RootD 'servers\QAcopy-1.21.1'))

    Type-Text (Find-ById 'server-delete-confirm') 'QAcopy-1.21.1'
    Start-Sleep -Milliseconds 600
    $sub = Find-ById 'server-delete-submit'
    Say ("  exact id: 삭제 enabled=$($sub.Current.IsEnabled)")
    Check '삭제 enables on the exact directory name' $sub.Current.IsEnabled
    [void](Save-WindowShot (Join-Path $Shots 'mods-09-delete-armed.png'))
    [void](Invoke-Control $sub)
    $toast = @()
    for ($i = 0; $i -lt 80 -and $toast.Count -eq 0; $i++) {
        $toast = @(Find-TextLike '서버를 삭제했습니다') + @(Find-TextLike '삭제 실패')
        if ($toast.Count -eq 0) { Start-Sleep -Milliseconds 250 }
    }
    Say ("  toast: " + (($toast | Select-Object -Unique) -join ' | '))
    Start-Sleep -Seconds 3
    Say ("  disk servers: " + ((Get-ChildItem (Join-Path $RootD 'servers') -Directory | ForEach-Object { $_.Name }) -join ', '))
    Check 'the server directory is gone' (-not (Test-Path (Join-Path $RootD 'servers\QAcopy-1.21.1')))
    Check 'its row is gone' ($null -eq (Find-ById 'servers-menu-QAcopy-1.21.1' -Optional -TimeoutSeconds 4))
    [void](Save-WindowShot (Join-Path $Shots 'mods-09-deleted.png'))
}
finally { Stop-App $proc }
End-Log
