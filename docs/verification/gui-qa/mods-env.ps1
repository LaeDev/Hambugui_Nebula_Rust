# env.ps1 — one launch/close contract shared by every 모드 scenario script.
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$script:Base  = 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\gui-qa'
$Global:Base  = $script:Base
$Global:Exe   = Join-Path $script:Base 'target\release\nebula.exe'
$Global:RootD = Join-Path $script:Base 'root'
$Global:Cfg   = Join-Path $script:Base 'config'
$Global:Shots = 'H:\nebularust\docs\verification\gui-qa'

. (Join-Path $script:Base 'drive-mods.ps1')

function Start-App([string] $LogName = 'app') {
    $env:ROOT = $Global:RootD
    $env:BASE_URL = 'https://cdn.example.invalid/'
    $env:NEBULA_CONFIG_DIR = $Global:Cfg
    $env:RUST_LOG = 'info'
    Remove-Item Env:\NEBULA_CURSEFORGE_API_KEY -ErrorAction SilentlyContinue
    $out = Join-Path $Global:Shots "$LogName-stdout.log"
    $proc = Start-Process -FilePath $Global:Exe -ArgumentList 'gui' -PassThru `
        -RedirectStandardOutput $out -RedirectStandardError "$out.err"
    [void](Connect-App -ProcessId $proc.Id)
    $proc
}

function Stop-App($proc) {
    try { Close-AppWindow } catch { }
    Start-Sleep -Seconds 2
    if ($proc -and -not $proc.HasExited) { $proc.Kill() }
}
