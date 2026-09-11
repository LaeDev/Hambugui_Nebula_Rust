$ErrorActionPreference = 'Stop'
. 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\gui-qa\env.ps1'

Start-Log $Shots 's04c-more'
$proc = Start-App 's04c'
try {
    Start-Sleep -Seconds 4
    Send-Keys '^3'
    Start-Sleep -Seconds 3
    [void](Invoke-Control (Find-ById 'mods-view-1'))
    Start-Sleep -Seconds 1
    Type-Text (Find-ById 'mods-search') 'sodium'
    Send-Keys '{ENTER}'
    Start-Sleep -Seconds 4

    Say '--- reach 더 보기 with the wheel ---'
    for ($i = 0; $i -lt 10; $i++) {
        Scroll-Page 6
        $n = (Find-ByIdPrefix 'mods-install-').Count
        $m = Find-ById 'mods-search-more' -Optional -TimeoutSeconds 1
        Say ("  notch block $i : visible install buttons=$n  more=$($null -ne $m)")
        if ($m) { break }
    }
    $more = Find-ById 'mods-search-more' -Optional -TimeoutSeconds 2
    Check '더 보기 exists at the bottom of a full page' ($null -ne $more)
    if ($more) {
        Say ("  button: '$($more.Current.Name)' enabled=$($more.Current.IsEnabled)")
        $ids0 = @(Find-ByIdPrefix 'mods-install-' | ForEach-Object { $_.Current.AutomationId })
        [void](Save-WindowShot (Join-Path $Shots 'mods-04-more-before.png'))
        $focusBefore = Focused-Desc
        [void](Invoke-Control $more)
        Start-Sleep -Seconds 5
        $ids1 = @(Find-ByIdPrefix 'mods-install-' | ForEach-Object { $_.Current.AutomationId })
        Say ("  before: " + ($ids0 -join ','))
        Say ("  after : " + ($ids1 -join ','))
        $kept = @($ids0 | Where-Object { $ids1 -contains $_ })
        $new  = @($ids1 | Where-Object { $ids0 -notcontains $_ })
        Say ("  kept=$($kept.Count) new=$($new.Count)")
        Check '더 보기 appended rows' ($new.Count -gt 0) ("new=" + ($new -join ','))
        [void](Save-WindowShot (Join-Path $Shots 'mods-04-more-after.png'))
        Say ("  focus before/after: $focusBefore  ->  " + (Focused-Desc))
        Say ("  더 보기 still there: " + ($null -ne (Find-ById 'mods-search-more' -Optional -TimeoutSeconds 2)))
    }
}
finally { Stop-App $proc }
End-Log
