$ErrorActionPreference = 'Stop'
. 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\gui-qa\env.ps1'

Start-Log $Shots 's04-search'
$proc = Start-App 's04'
try {
    Start-Sleep -Seconds 4
    Send-Keys '^3'
    Start-Sleep -Seconds 3
    Say ("scope: " + ((Find-TextLike ' · Minecraft ') -join ' | '))

    Say '--- 4 찾기 ---'
    [void](Invoke-Control (Find-ById 'mods-view-1'))
    Start-Sleep -Seconds 1
    Say '  --- pre-search ---'
    foreach ($t in (Get-Texts)) { Say ("    " + $t) }
    Say ("  ids: " + ((Get-Ids | Where-Object { $_ -like 'mods-*' }) -join ' ; '))
    [void](Save-WindowShot (Join-Path $Shots 'mods-04-presearch.png'))

    $cf = Find-ById 'mods-provider-1' -Optional
    if ($cf) { Say ("  CurseForge segment: name='$($cf.Current.Name)' enabled=$($cf.Current.IsEnabled)") }

    $field = Find-ById 'mods-search'
    Type-Text $field 'sodium'
    Start-Sleep -Milliseconds 300
    Send-Keys '{ENTER}'
    $t0 = Get-Date
    [void](Wait-Until { (Find-ByIdPrefix 'mods-install-').Count -gt 0 } 30 'results')
    $elapsed = ((Get-Date) - $t0).TotalSeconds
    Say ("  first results after {0:N1}s" -f $elapsed)
    $shotA = Save-WindowShot (Join-Path $Shots 'mods-04-results-noicons.png')

    Say '  --- result rows ---'
    foreach ($t in (Get-Texts)) { Say ("    " + $t) }
    $btns = @(Find-ByIdPrefix 'mods-install-')
    Say ("  install buttons: " + (($btns | ForEach-Object { "$($_.Current.AutomationId)='$($_.Current.Name)' enabled=$($_.Current.IsEnabled)" }) -join ' ; '))
    Check 'search returned rows' ($btns.Count -gt 0) "count=$($btns.Count)"
    Check 'ids follow mods-install-modrinth-<project id>' (($btns | Where-Object { $_.Current.AutomationId -like 'mods-install-modrinth-*' }).Count -eq $btns.Count)
    $caps = @(Find-TextLike '다운로드 ')
    Say ("  captions: " + ($caps -join ' | '))
    Check 'captions carry 다운로드 N' ($caps.Count -gt 0)
    Check 'captions carry 수정 N' ((@(Find-TextLike '수정 ')).Count -gt 0)

    # Icons are fetched by a bounded queue after the rows land.
    Start-Sleep -Seconds 6
    $shotB = Save-WindowShot (Join-Path $Shots 'mods-04-results-icons.png')
    $diff = Compare-Shots $shotA $shotB
    Say ("  whole-window diff after 6 s of icon fetching: $diff")
    Check 'the rows changed once the icons arrived' ($diff -gt 0.1) "diff=$diff"

    # 더 보기
    Say '--- 더 보기 ---'
    $more = Find-ById 'mods-search-more' -Optional -TimeoutSeconds 3
    if ($more) {
        $before = (Find-ByIdPrefix 'mods-install-').Count
        Say ("  rows before: $before  button='$($more.Current.Name)'")
        [void](Invoke-Control $more)
        [void](Wait-Until { (Find-ByIdPrefix 'mods-install-').Count -gt $before } 30 'more rows')
        $after = (Find-ByIdPrefix 'mods-install-').Count
        Say ("  rows after: $after")
        Check '더 보기 appends rows' ($after -gt $before) "$before -> $after"
        [void](Save-WindowShot (Join-Path $Shots 'mods-04-more.png'))
    } else {
        Check '더 보기 is present when the page was full' $false 'mods-search-more absent'
    }

    # sort
    Say '--- sort ---'
    $sorts = @(Find-ByIdPrefix 'mods-sort-')
    Say ("  sort options: " + (($sorts | ForEach-Object { "$($_.Current.AutomationId)='$($_.Current.Name)'" }) -join ' ; '))
    $firstRelevance = (@(Find-ByIdPrefix 'mods-install-') | Select-Object -First 1).Current.AutomationId
    Say ("  first row (관련도): $firstRelevance")

    [void](Invoke-Control (Find-ById 'mods-sort-1'))
    Start-Sleep -Seconds 1
    [void](Wait-Until { (Find-ByIdPrefix 'mods-install-').Count -gt 0 } 30 'downloads results')
    Start-Sleep -Seconds 2
    $firstDownloads = (@(Find-ByIdPrefix 'mods-install-') | Select-Object -First 1).Current.AutomationId
    $n = (Find-ByIdPrefix 'mods-install-').Count
    Say ("  first row (다운로드): $firstDownloads  rows=$n")
    Check '다운로드 re-searched (offset reset to one page)' ($n -le 20) "rows=$n"
    [void](Save-WindowShot (Join-Path $Shots 'mods-04-sort-downloads.png'))

    [void](Invoke-Control (Find-ById 'mods-sort-2'))
    Start-Sleep -Seconds 1
    [void](Wait-Until { (Find-ByIdPrefix 'mods-install-').Count -gt 0 } 30 'newest results')
    Start-Sleep -Seconds 2
    Say ("  first row (최신): " + (@(Find-ByIdPrefix 'mods-install-') | Select-Object -First 1).Current.AutomationId)
    Say '  --- 최신 captions (date first per §8.4) ---'
    foreach ($t in (Find-TextLike '다운로드 ')) { Say ("    " + $t) }
    [void](Save-WindowShot (Join-Path $Shots 'mods-04-sort-newest.png'))

    # back to 관련도 and re-search sodium for the install scenario
    [void](Invoke-Control (Find-ById 'mods-sort-0'))
    Start-Sleep -Seconds 3
}
finally { Stop-App $proc }
End-Log
