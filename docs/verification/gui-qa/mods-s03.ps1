$ErrorActionPreference = 'Stop'
. 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\gui-qa\env.ps1'

Start-Log $Shots 's03-mod-detail'
$proc = Start-App 's03'
try {
    Start-Sleep -Seconds 4
    Send-Keys '^3'
    Start-Sleep -Seconds 3
    Say ("scope: " + ((Find-TextLike ' · Minecraft ') -join ' | '))

    Say '--- 3 mod-detail via the row chevron ---'
    $jar = 'cloth-config-15.0.140-neoforge.jar'
    $chev = Find-ById "mods-row-$jar"
    Say ("  chevron: name='$($chev.Current.Name)' role=$($chev.Current.ControlType.ProgrammaticName)")
    [void](Invoke-Control $chev)
    Start-Sleep -Seconds 1

    Say '  --- dialog texts ---'
    foreach ($t in (Get-Texts)) { Say ("    " + $t) }
    Say ("  ids: " + ((Get-Ids) -join ' ; '))
    Say ("  initial focus: " + (Focused-Desc))
    [void](Save-WindowShot (Join-Path $Shots 'mods-03-detail.png'))

    Check '다른 버전… is hidden for 직접 추가' ($null -eq (Find-ById 'mod-detail-versions' -Optional -TimeoutSeconds 2))
    Check '프로젝트 페이지 열기 is hidden without a provider id' ($null -eq (Find-ById 'mod-detail-project' -Optional -TimeoutSeconds 2))
    Check 'initial focus is the namespace control' ((Focused-Id) -like 'mod-detail-namespace*') (Focused-Id)

    $seg = @(Find-ByIdPrefix 'mod-detail-namespace-')
    Say ("  namespace segment: " + (($seg | ForEach-Object { "$($_.Current.AutomationId)='$($_.Current.Name)' sel=$(Get-Selected $_)" }) -join ' ; '))

    # 선택 꺼짐 -> 선택 켜짐 (index 1). The control stages; 이동 applies.
    $target = Find-ById 'mod-detail-namespace-1'
    [void](Invoke-Control $target)
    Start-Sleep -Milliseconds 700
    Say ("  after staging: " + ((Find-ByIdPrefix 'mod-detail-namespace-' | ForEach-Object { "$($_.Current.AutomationId)=$(Get-Selected $_)" }) -join ' '))
    [void](Save-WindowShot (Join-Path $Shots 'mods-03-detail-staged.png'))

    $apply = Find-ById 'mod-detail-apply'
    Say ("  primary: name='$($apply.Current.Name)' enabled=$($apply.Current.IsEnabled)")
    [void](Invoke-Control $apply)
    Start-Sleep -Seconds 3

    $toast = @(Find-TextLike '이동했습니다')
    Say ("  toast: " + ($toast -join ' | '))
    Check 'the move toast names the target namespace' ($toast.Count -gt 0) ($toast -join ' | ')
    [void](Save-WindowShot (Join-Path $Shots 'mods-03-moved.png'))

    Start-Sleep -Seconds 2
    Say '  --- list after the move ---'
    foreach ($t in (Get-Texts)) { Say ("    " + $t) }
    Check 'the dialog closed' ($null -eq (Find-ById 'mod-detail-apply' -Optional -TimeoutSeconds 2))
    Check 'the row now reads 선택 켜짐' (Has-Text '선택 켜짐')
    Say ("  focus after close: " + (Focused-Desc))

    # ground truth on disk
    $ns = Join-Path $RootD 'servers\QAfab-1.20.1\fabricmods'
    foreach ($d in 'required','optionalon','optionaloff') {
        Say ("  disk $d : " + ((Get-ChildItem (Join-Path $ns $d) -ErrorAction SilentlyContinue | ForEach-Object { $_.Name }) -join ', '))
    }
}
finally { Stop-App $proc }
End-Log
