$ErrorActionPreference = 'Stop'
. 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\gui-qa\env.ps1'

# s10b's probe pressed keys while measuring. Here nothing is pressed: just a
# 100 ms poll of the busy indicators for the whole life of one long job, so
# "when did busy stop" is answered before any interaction is blamed for it.
$pad = Join-Path $RootD 'servers\QAneo-1.21.1\files\pad.bin'
Start-Log $Shots 's10c-busy-timeline'
if (-not (Test-Path $pad)) {
    # Real bytes, not SetLength: a sparse file copies in milliseconds and the
    # first attempt at this measurement mistook that for the busy flag clearing.
    $fs = [System.IO.File]::Create($pad)
    $buf = New-Object byte[] (8MB)
    (New-Object Random 1).NextBytes($buf)
    for ($k = 0; $k -lt 75; $k++) { $fs.Write($buf, 0, $buf.Length) }
    $fs.Close()
}
Say ("pad: " + (Get-Item $pad).Length + " B")

$proc = Start-App 's10c'
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

    $t0 = Get-Date
    [void](Invoke-Control (Find-ById 'server-duplicate-submit'))
    Say '--- timeline, no keys pressed ---'
    for ($i = 0; $i -lt 200; $i++) {
        $d = Find-ById 'server-duplicate-submit' -Optional -TimeoutSeconds 0
        $new = Find-ById 'servers-new' -Optional -TimeoutSeconds 0
        $pill = @(Get-Texts) | Where-Object { $_ -like '*복제 중*' -or $_ -like '*복제했습니다*' }
        Say ("  [{0,5:N1}s] dialog={1} submit-enabled={2} servers-new-enabled={3} pill/toast='{4}'" -f `
            ((Get-Date) - $t0).TotalSeconds, ($null -ne $d),
            $(if ($d) { $d.Current.IsEnabled } else { 'n/a' }),
            $(if ($new) { $new.Current.IsEnabled } else { 'n/a' }),
            (($pill | Select-Object -Unique) -join ' | '))
        if ((Test-Path (Join-Path $RootD 'servers\QAbusy-1.21.1')) -and $null -eq $d) { Say '  (the copy landed and the dialog is gone)'; break }
        Start-Sleep -Milliseconds 400
    }
    Say ("  copy present: " + (Test-Path (Join-Path $RootD 'servers\QAbusy-1.21.1')))
    Say ("  partials: " + ((Get-ChildItem (Join-Path $RootD 'servers') -Force -Directory | ForEach-Object { $_.Name }) -join ', '))
}
finally {
    Stop-App $proc
    Remove-Item -Recurse -Force (Join-Path $RootD 'servers\QAbusy-1.21.1') -ErrorAction SilentlyContinue
    Get-ChildItem (Join-Path $RootD 'servers') -Force -Directory | Where-Object { $_.Name -like '.*partial*' } | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -Force $pad -ErrorAction SilentlyContinue
    Say 'cleaned up'
}
End-Log
