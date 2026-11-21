<#
Builds the Windows release artefacts locally. Nothing is published or signed.
  pwsh work/release/build.ps1 [-SkipBuild] [-NoMsi]
Output (in dist/, which git ignores):
  dist/Math++-<version>-portable.zip   Math++.exe + mpp.exe + docs + examples + licences
  dist/Math++-<version>-x64.msi        per-user installer (no admin rights needed)
Also writes licences\THIRD-PARTY-NOTICES.txt (every linked crate's licence text, via third-party-notices.ps1).
Needs: cargo; WiX Toolset 3.x (candle.exe/light.exe) for the .msi. WiX is found from $env:WIX_BIN,
$env:WIX (its bin folder), target/tools/wix3 (the unzipped wix314-binaries.zip) or the PATH.
#>
param([switch]$SkipBuild, [switch]$NoMsi)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location $root
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

# Version comes from Cargo.toml ([workspace.package] version).
$version = (Select-String -Path Cargo.toml -Pattern '^\s*version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
Write-Host "Math++ $version"

if (-not $SkipBuild) {
    cargo build --release --workspace
    if ($LASTEXITCODE) { throw 'cargo build failed' }
}
$rel = Join-Path $root 'target\release'
foreach ($f in 'mathpp.exe', 'mpp.exe') { if (-not (Test-Path "$rel\$f")) { throw "missing $rel\$f" } }

# Stage. Cargo bin names cannot contain '+', so the IDE is built as mathpp.exe and renamed here.
$dist = Join-Path $root 'dist'
$stage = Join-Path $dist "stage\Math++-$version"
if (Test-Path "$dist\stage") { Remove-Item "$dist\stage" -Recurse -Force }
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item "$rel\mathpp.exe" "$stage\Math++.exe"
Copy-Item "$rel\mpp.exe" "$stage\mpp.exe"
Copy-Item LICENSE-MIT, LICENSE-APACHE, CHANGELOG.mpp, README.mpp $stage
Copy-Item work\release\licences "$stage\licences" -Recurse
# Licence notices for every Rust crate linked into the binaries (required by their licences).
& (Join-Path $PSScriptRoot 'third-party-notices.ps1') -Output "$stage\licences\THIRD-PARTY-NOTICES.txt"
if (-not (Test-Path "$stage\licences\THIRD-PARTY-NOTICES.txt")) { throw 'third-party notices were not written' }
Copy-Item docs "$stage\docs" -Recurse
Copy-Item examples "$stage\examples" -Recurse

$zip = "$dist\Math++-$version-portable.zip"
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path $stage -DestinationPath $zip
Write-Host "wrote $zip"
if ($NoMsi) { return }

# Locate WiX 3.
$wix = @($env:WIX_BIN, $(if ($env:WIX) { Join-Path $env:WIX 'bin' }), "$root\target\tools\wix3") |
    Where-Object { $_ -and (Test-Path "$_\candle.exe") } | Select-Object -First 1
if (-not $wix) {
    $c = Get-Command candle.exe -ErrorAction SilentlyContinue
    if ($c) { $wix = Split-Path $c.Source }
}
if (-not $wix) { throw 'WiX 3 not found: unzip wix314-binaries.zip to target\tools\wix3 or set WIX_BIN' }

# Licence page text (MIT shown; Apache-2.0 sits in the install folder too).
$mit = (Get-Content LICENSE-MIT -Raw).Replace('\', '\\').Replace('{', '\{').Replace('}', '\}') -replace "`r?`n", '\par '
$rtf = "$dist\license.rtf"
Set-Content $rtf ('{\rtf1\ansi\deff0{\fonttbl{\f0 Segoe UI;}}\fs20 Math++ is available under the MIT or Apache-2.0 licence, at your option. Both are in the install folder.\par\par ' + $mit + '}') -Encoding ASCII

# Fragment with every other staged file (docs, examples, licences...), one component each.
function Get-Hash([string]$p) { [BitConverter]::ToString([Security.Cryptography.MD5]::Create().ComputeHash([Text.Encoding]::UTF8.GetBytes($p))).Replace('-', '') }
function Get-Guid([string]$p) { [Guid]::new([Security.Cryptography.MD5]::Create().ComputeHash([Text.Encoding]::UTF8.GetBytes("mathpp:$p"))).ToString().ToUpper() }
$sb = [Text.StringBuilder]::new()
$comps = New-Object Collections.Generic.List[string]
function Add-Dir([string]$dir, [string]$rel, [int]$depth) {
    $pad = '  ' * $depth
    foreach ($f in Get-ChildItem $dir -File) {
        $r = if ($rel) { "$rel\$($f.Name)" } else { $f.Name }
        if ($r -in 'Math++.exe', 'mpp.exe') { continue }
        $id = 'N' + (Get-Hash $r); $comps.Add($id)
        $name = [Security.SecurityElement]::Escape($f.Name); $src = [Security.SecurityElement]::Escape($f.FullName)
        $null = $sb.AppendLine("$pad<Component Id=`"$id`" Guid=`"$(Get-Guid $r)`"><File Id=`"F$id`" Name=`"$name`" Source=`"$src`" KeyPath=`"yes`" /></Component>")
    }
    foreach ($d in Get-ChildItem $dir -Directory) {
        $r = if ($rel) { "$rel\$($d.Name)" } else { $d.Name }
        $null = $sb.AppendLine("$pad<Directory Id=`"D$(Get-Hash $r)`" Name=`"$([Security.SecurityElement]::Escape($d.Name))`">")
        Add-Dir $d.FullName $r ($depth + 1)
        $null = $sb.AppendLine("$pad</Directory>")
    }
}
Add-Dir $stage '' 1
$refs = ($comps | ForEach-Object { "  <ComponentRef Id=`"$_`" />" }) -join "`n"
$frag = "<?xml version=`"1.0`" encoding=`"UTF-8`"?>`n<Wix xmlns=`"http://schemas.microsoft.com/wix/2006/wi`"><Fragment>`n<DirectoryRef Id=`"INSTALLFOLDER`">`n$($sb.ToString())</DirectoryRef>`n<ComponentGroup Id=`"StageFiles`">`n$refs`n</ComponentGroup></Fragment></Wix>"
Set-Content "$dist\stagefiles.wxs" $frag -Encoding UTF8

$ico = Join-Path $root 'crates\mpp-ide\assets\mathpp.ico'
$msi = "$dist\Math++-$version-x64.msi"
& "$wix\candle.exe" -nologo -arch x64 -ext WixUIExtension "-dVersion=$version" "-dStage=$stage" "-dIco=$ico" "-dRtf=$rtf" -out "$dist\obj\" work\release\mathpp.wxs "$dist\stagefiles.wxs"
if ($LASTEXITCODE) { throw 'candle failed' }
# ICE38/64/91 complain about per-user components; this installer is deliberately per-user.
& "$wix\light.exe" -nologo -ext WixUIExtension -sice:ICE38 -sice:ICE64 -sice:ICE91 -out $msi "$dist\obj\mathpp.wixobj" "$dist\obj\stagefiles.wixobj"
if ($LASTEXITCODE) { throw 'light failed' }
Write-Host "wrote $msi"
