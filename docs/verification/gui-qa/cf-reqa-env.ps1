$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$Global:Base  = 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\cf-reqa'
$Global:Exe   = Join-Path $Global:Base 'target\release\nebula.exe'
$Global:RootD = Join-Path $Global:Base 'root'
$Global:Cfg   = Join-Path $Global:Base 'config-gui'
$Global:Shots = 'H:\nebularust\docs\verification\gui-qa'
$Global:KeyFile = 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\cf-key.txt'

. (Join-Path $Global:Base 'gui\drive-mods.ps1')

function Start-App([string] $LogName = 'app', [switch] $NoKey, [string] $Root = $null) {
    $env:ROOT = if ($Root) { $Root } else { $Global:RootD }
    $env:BASE_URL = 'https://cdn.example.invalid/'
    $env:NEBULA_CONFIG_DIR = $Global:Cfg
    $env:RUST_LOG = 'info'
    Remove-Item Env:\NEBULA_CURSEFORGE_API_KEY -ErrorAction SilentlyContinue
    if (-not $NoKey) {
        $env:NEBULA_CURSEFORGE_API_KEY = (Get-Content $Global:KeyFile -Raw).Trim()
    }
    $out = Join-Path $Global:Base "logs\$LogName-stdout.log"
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
