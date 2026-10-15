$ErrorActionPreference = 'Stop'
. 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\gui-qa\env.ps1'

Start-Log $Shots 's02-installed'
$proc = Start-App 's02'
try {
    Start-Sleep -Seconds 4
    Send-Keys '^3'
    Start-Sleep -Seconds 3
    Check 'on 모드' ((Get-PageTitle) -eq '모드') (Get-PageTitle)
    Say ("scope: " + ((Find-TextLike ' · Minecraft ') -join ' | '))
    Say ("persisted server = the one s01 left selected (last-mods-server)")

    # ── how a radio pill reports its state to UIA ──────────────────────────
    Say '--- pill UIA state carriers ---'
    foreach ($id in 'mods-server-0','mods-server-1') {
        $e = Find-ById $id
        Say ("  {0}: patterns=[{1}] toggle={2} selected={3}" -f $id, (Get-Patterns $e), (Get-Checked $e), (Get-Selected $e))
    }
    $grp = Find-ById 'mods-server'
    Say ("  mods-server: role=$($grp.Current.ControlType.ProgrammaticName) patterns=[$(Get-Patterns $grp)]")

    # ── 2. 설치됨 list ─────────────────────────────────────────────────────
    Say '--- 2 installed list ---'
    [void](Save-WindowShot (Join-Path $Shots 'mods-02-installed.png'))
    Say '  --- every Text on the card ---'
    foreach ($t in (Get-Texts)) { Say ("    " + $t) }

    Check 'the hand-placed jar has a row' ($null -ne (Find-ById 'mods-row-cloth-config-15.0.140-neoforge.jar' -Optional))
    Check 'notes.txt has a row'          ($null -ne (Find-ById 'mods-row-notes.txt' -Optional))
    Check 'the jar is marked 직접 추가'   (Has-Text '직접 추가')
    Check 'notes.txt is marked 배포 불가' (Has-Text '배포 불가')
    Check 'the 배포 불가 helper is on screen' (Has-Text '배포본을 만들 수 없는 파일입니다')
    Say ("  header subtitle: " + ((Find-TextLike '관리 ') -join ' | '))
    Say ("  title: " + ((Find-TextLike '설치된 모드') -join ' | '))
    Say ("  subtitles: " + ((Find-TextLike '.jar') -join ' | '))
    Say ("  namespace chips: " + ((@('필수','선택 켜짐','선택 꺼짐') | ForEach-Object { if (Has-Text $_) { $_ } }) -join ', '))

    # HelpText is the §6 ceiling; record what it actually returns.
    $row = Find-ById 'mods-row-notes.txt'
    Say ("  notes row HelpText=[" + (Get-Description $row) + "] FullDescription=[" + (Get-FullDescription $row) + "]")
}
finally { Stop-App $proc }
End-Log
