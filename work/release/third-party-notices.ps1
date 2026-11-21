<#
Writes THIRD-PARTY-NOTICES.txt: the name, version, licence and licence text of every Rust crate
that is linked into Math++.exe and mpp.exe. Run by build.ps1; an equivalent of cargo-about built
on `cargo metadata`, so it needs nothing but cargo.
  pwsh work/release/third-party-notices.ps1 -Output dist\THIRD-PARTY-NOTICES.txt [-Target x86_64-pc-windows-msvc]
Only crates reachable through normal (non-dev, non-build) dependencies are listed. The text of each
distinct licence file is printed once, followed by the crates it belongs to.
#>
param(
    [Parameter(Mandatory)][string]$Output,
    [string]$Target = 'x86_64-pc-windows-msvc'
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location $root

$json = cargo metadata --format-version 1 --locked --filter-platform $Target
if ($LASTEXITCODE) { throw 'cargo metadata failed' }
$meta = ($json -join "`n") | ConvertFrom-Json
$byId = @{}
foreach ($p in $meta.packages) { $byId[$p.id] = $p }
$nodes = @{}
foreach ($n in $meta.resolve.nodes) { $nodes[$n.id] = $n }
$members = [Collections.Generic.HashSet[string]]::new([string[]]$meta.workspace_members)

# Everything reachable from the workspace members through normal dependencies.
$seen = [Collections.Generic.HashSet[string]]::new()
$queue = [Collections.Generic.Queue[string]]::new()
foreach ($m in $members) { $null = $seen.Add($m); $queue.Enqueue($m) }
while ($queue.Count) {
    $id = $queue.Dequeue()
    foreach ($d in $nodes[$id].deps) {
        $normal = $d.dep_kinds | Where-Object { $null -eq $_.kind }
        if ($normal -and $seen.Add($d.pkg)) { $queue.Enqueue($d.pkg) }
    }
}

# The standard text of each licence a crate names but ships no file for, with its authors as
# the copyright holders (MIT, BSL and zlib require the notice; the templates are in spdx\).
$spdxDir = Join-Path $PSScriptRoot 'spdx'
function Get-SpdxText($p, [switch]$Known) {
    $holders = if ($p.authors) { ($p.authors | ForEach-Object { $_ -replace '\s*<[^>]*>', '' }) -join ', ' } else { "the $($p.name) authors" }
    $ids = [regex]::Matches($p.license, '[A-Za-z0-9.\-]+') | ForEach-Object { $_.Value } |
        Where-Object { $_ -notin 'OR', 'AND', 'WITH' } | Select-Object -Unique
    $parts = foreach ($id in $ids) {
        $file = if ($id -eq 'Apache-2.0') { Join-Path $root 'LICENSE-APACHE' } else { Join-Path $spdxDir "$id.txt" }
        if (-not (Test-Path $file)) {
            if ($Known) { continue }
            throw "$($p.name) $($p.version) ships no licence file and there is no standard text for $id in $spdxDir"
        }
        $body = ((Get-Content $file -Raw) -replace "`r`n", "`n").Trim().Replace('{copyright}', $holders)
        "--- $id (standard text; the crate ships no licence file) ---`n$body"
    }
    $parts -join "`n`n"
}

$sha = [Security.Cryptography.SHA256]::Create()
$groups = @{}
foreach ($id in $seen) {
    if ($members.Contains($id)) { continue }
    $p = $byId[$id]
    $dir = Split-Path $p.manifest_path
    $files = @(Get-ChildItem $dir -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match '^(LICEN[CS]E|COPYING|NOTICE|UNLICENSE)' } | Sort-Object Name)
    if (-not $files) {
        # Licences kept beside what they cover, one folder down (fonts/OFL.txt, fonts/UFL.txt …).
        $files = @(Get-ChildItem $dir -Directory -ErrorAction SilentlyContinue | ForEach-Object {
                Get-ChildItem $_.FullName -File -ErrorAction SilentlyContinue |
                    Where-Object { $_.Name -match '(?i)(^(OFL|UFL)\b|licen[cs]e|copying)' }
            } | Sort-Object Name)
    }
    $text = ($files | ForEach-Object { "--- $($_.Name) ---`n" + ((Get-Content $_.FullName -Raw) -replace "`r`n", "`n").Trim() }) -join "`n`n"
    if (-not $text) {
        $text = Get-SpdxText $p
    } elseif ($files[0].DirectoryName -ne $dir) {
        # Only the licences of bundled files were found: add the crate's own licence.
        $own = Get-SpdxText $p -Known
        if ($own) { $text = "$own`n`n$text" }
    }
    $key = [BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($text)))
    if (-not $groups.ContainsKey($key)) { $groups[$key] = @{ Text = $text; Crates = [Collections.Generic.List[object]]::new() } }
    $groups[$key].Crates.Add($p)
}

$sb = [Text.StringBuilder]::new()
$count = ($groups.Values | ForEach-Object { $_.Crates.Count } | Measure-Object -Sum).Sum
$null = $sb.AppendLine("Math++ third-party notices")
$null = $sb.AppendLine("Math++ itself is MIT OR Apache-2.0 (LICENSE-MIT, LICENSE-APACHE). It links $count Rust crates,")
$null = $sb.AppendLine("each under the licence below. Bundled fonts have their own notices in the licences folder.")
$null = $sb.AppendLine()
foreach ($g in $groups.Values | Sort-Object { $_.Crates[0].name }) {
    $null = $sb.AppendLine(('=' * 78))
    foreach ($c in $g.Crates | Sort-Object name, version) {
        $line = "$($c.name) $($c.version)  [$($c.license)]"
        if ($c.repository) { $line += "  $($c.repository)" }
        $null = $sb.AppendLine($line)
    }
    $null = $sb.AppendLine(('-' * 78))
    $null = $sb.AppendLine($g.Text)
    $null = $sb.AppendLine()
}
$out = if ([IO.Path]::IsPathRooted($Output)) { $Output } else { Join-Path $root $Output }
New-Item -ItemType Directory -Force (Split-Path $out) | Out-Null
[IO.File]::WriteAllText($out, $sb.ToString(), [Text.UTF8Encoding]::new($false))
Write-Host "wrote $out ($count crates, $($groups.Count) distinct licence texts)"
