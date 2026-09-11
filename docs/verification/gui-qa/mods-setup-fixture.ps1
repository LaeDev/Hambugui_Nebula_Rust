# setup-fixture.ps1 — builds the QA root with the same binary the GUI runs.
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$base = 'C:\Users\Lae_\AppData\Local\Temp\nebula-qa\gui-qa'
$exe  = Join-Path $base 'target\release\nebula.exe'
$root = Join-Path $base 'root'
$cfg  = Join-Path $base 'config'
$log  = Join-Path $base 'setup.log'

Remove-Item -Recurse -Force $root, $cfg -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $root, $cfg | Out-Null

$env:ROOT = $root
$env:BASE_URL = 'https://cdn.example.invalid/'
$env:NEBULA_CONFIG_DIR = $cfg
$env:RUST_LOG = 'info'
Remove-Item Env:\NEBULA_CURSEFORGE_API_KEY -ErrorAction SilentlyContinue

function Run([string[]] $ArgList) {
    Write-Host "`n>>> nebula $($ArgList -join ' ')"
    Add-Content -LiteralPath $log -Value "`n>>> nebula $($ArgList -join ' ')" -Encoding UTF8
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $out = & $exe @ArgList 2>&1 | Out-String
    $ErrorActionPreference = $prev
    Write-Host $out
    Add-Content -LiteralPath $log -Value $out -Encoding UTF8
    Add-Content -LiteralPath $log -Value "exit=$LASTEXITCODE" -Encoding UTF8
    Write-Host "exit=$LASTEXITCODE"
    $LASTEXITCODE
}

Set-Content -LiteralPath $log -Value "# setup  $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')" -Encoding UTF8

Run @('init','root')                                              | Out-Null
Run @('generate','server','QAfab','1.20.1','--fabric','latest')   | Out-Null
Run @('generate','server','QAneo','1.21.1','--neoforge','latest') | Out-Null

# A managed mod on the *other* server, only so its jar can be hand-placed on the
# Fabric one. cloth-config is deliberately not a Sodium dependency: a hand-placed
# fabric-api would satisfy D6 and turn scenario 5's pre-checked dependency switch
# into a disabled 설치됨 row, testing a different branch than the scenario names.
Run @('mods','QAneo-1.21.1','install','cloth-config','--namespace','required') | Out-Null

$src = Get-ChildItem (Join-Path $root 'servers\QAneo-1.21.1\neoforgemods\required') -Filter *.jar |
       Select-Object -First 1
if (-not $src) { throw 'cloth-config did not land on QAneo' }

$dst = Join-Path $root 'servers\QAfab-1.20.1\fabricmods\optionaloff'
New-Item -ItemType Directory -Force -Path $dst | Out-Null
Copy-Item $src.FullName (Join-Path $dst $src.Name)

# A file the distribution scanners abort on (§8.3 배포 불가).
$req = Join-Path $root 'servers\QAfab-1.20.1\fabricmods\required'
New-Item -ItemType Directory -Force -Path $req | Out-Null
Set-Content -LiteralPath (Join-Path $req 'notes.txt') -Value 'QA fixture: not a jar.' -Encoding UTF8

Write-Host "`n--- tree ---"
Get-ChildItem -Recurse $root | ForEach-Object {
    $_.FullName.Substring($root.Length + 1) + $(if ($_.PSIsContainer) { '\' } else { "  ($($_.Length) B)" })
} | Tee-Object -FilePath (Join-Path $base 'fixture-tree.txt')

# Prove the hand-placed jar really is a zip with mod metadata, or scenario 2
# measures 배포 불가 instead of 직접 추가.
Add-Type -AssemblyName System.IO.Compression.FileSystem
$z = [System.IO.Compression.ZipFile]::OpenRead((Join-Path $dst $src.Name))
$names = $z.Entries | ForEach-Object { $_.FullName } |
         Where-Object { $_ -match 'mod\.json|mods\.toml|mcmod\.info' }
$z.Dispose()
Write-Host "hand-placed jar: $($src.Name)  meta=[$($names -join ', ')]"
Add-Content -LiteralPath $log -Value "hand-placed jar: $($src.Name)  meta=[$($names -join ', ')]" -Encoding UTF8
