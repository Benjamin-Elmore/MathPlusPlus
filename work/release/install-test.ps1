<#
Installs the .msi per-user and silently, checks it, launches the IDE on a .mpp, uninstalls and checks
nothing is left. Run: pwsh work/release/install-test.ps1 > work/release/install-test.txt
#>
$ErrorActionPreference = 'Continue'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$msi = Get-ChildItem "$root\dist\Math++-*-x64.msi" | Select-Object -First 1
$dir = "$env:LOCALAPPDATA\Programs\Math++"
$lnk = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Math++.lnk"
$fail = 0
function Check([string]$what, [bool]$ok) { if (-not $ok) { $script:fail++ }; "{0,-4} {1}" -f $(if ($ok) { 'PASS' } else { 'FAIL' }), $what }
function Run([string]$cmd) { "> $cmd" }

"Math++ install test, $(Get-Date -Format s), whoami: $(whoami), admin: $(([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole('Administrator'))"
"msi: $($msi.Name) ($([math]::Round($msi.Length/1MB,1)) MB)"
Check 'not installed before' (-not (Test-Path $dir))

# 1. install (Main feature + the optional AddToPath feature)
Run "msiexec /i `"$($msi.Name)`" /qn /l*v dist\install.log ADDLOCAL=Main,AddToPath"
$p = Start-Process msiexec -ArgumentList "/i `"$($msi.FullName)`" /qn /l*v `"$root\dist\install.log`" ADDLOCAL=Main,AddToPath" -Wait -PassThru
Check "msiexec install exit code 0 (got $($p.ExitCode))" ($p.ExitCode -eq 0)

# 2. files, shortcut, registry
foreach ($f in 'Math++.exe', 'mpp.exe', 'LICENSE-MIT', 'LICENSE-APACHE', 'CHANGELOG.mpp', 'licences\GUST-Latin-Modern-LICENSE.txt', 'licences\OFL-egui-fonts.txt', 'docs\guide.mpp', 'examples\integrals.mpp', 'examples\imports') {
    Check "installed: $f" (Test-Path "$dir\$f")
}
Check 'Start-menu shortcut Math++.lnk' (Test-Path $lnk)
if (Test-Path $lnk) { $t = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk).TargetPath; Check "shortcut target = $t" ($t -eq "$dir\Math++.exe") }
$cls = 'HKCU:\Software\Classes'
Check ".mpp -> $((Get-ItemProperty "$cls\.mpp" -ErrorAction SilentlyContinue).'(default)')" ((Get-ItemProperty "$cls\.mpp" -ErrorAction SilentlyContinue).'(default)' -eq 'MathPlusPlus.Document')
$cmd = (Get-ItemProperty "$cls\MathPlusPlus.Document\shell\open\command" -ErrorAction SilentlyContinue).'(default)'
Check "open command: $cmd" ($cmd -eq "`"$dir\Math++.exe`" `"%1`"")
$ic = (Get-ItemProperty "$cls\MathPlusPlus.Document\DefaultIcon" -ErrorAction SilentlyContinue).'(default)'
Check "DefaultIcon: $ic" ($ic -eq "$dir\Math++.exe,0")
$up = [Environment]::GetEnvironmentVariable('Path', 'User')
Check 'user PATH contains the install folder' (($up -split ';') -contains ($dir + [char]92))
$vi = (Get-Item "$dir\Math++.exe").VersionInfo
Check "Math++.exe version info: $($vi.ProductName) $($vi.FileVersion)" ($vi.ProductName -eq 'Math++' -and $vi.FileVersion -like '0.1.0*')
$arp = Get-ItemProperty 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*' | Where-Object DisplayName -eq 'Math++'
Check "Apps & features entry (Windows lists per-user MSIs under HKLM Uninstall): $($arp.DisplayName) $($arp.DisplayVersion)" ($null -ne $arp)

# 3. CLI from the install folder
$out = & "$dir\mpp.exe" run "$dir\examples\integrals.mpp" 2>&1 | Out-String
Run "$dir\mpp.exe run examples\integrals.mpp"
Check 'mpp run works (prints 1/3)' ($out -match '1/3')

# 4. IDE opens a file given on the command line (what a double-click does)
$assoc = [regex]::Matches($cmd, '"([^"]+)"')
Run "Math++.exe examples\integrals.mpp   (command taken from the registry open command, %1 replaced)"
$exe = $assoc[0].Groups[1].Value
$ide = Start-Process $exe -ArgumentList "`"$dir\examples\integrals.mpp`"" -PassThru
Start-Sleep -Seconds 6
$ide.Refresh()
Check "IDE alive after 6 s (pid $($ide.Id), HasExited=$($ide.HasExited))" (-not $ide.HasExited)
Check "IDE has a window titled: $($ide.MainWindowTitle)" ($ide.MainWindowHandle -ne 0)
$b = [IO.File]::ReadAllBytes("$dir\Math++.exe"); $sub = [BitConverter]::ToUInt16($b, [BitConverter]::ToInt32($b, 0x3c) + 24 + 68)
Check "IDE has no console window (PE subsystem $sub, 2 = GUI)" ($sub -eq 2)
Stop-Process -Id $ide.Id -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 1

# 5. uninstall
Run "msiexec /x `"$($msi.Name)`" /qn /l*v dist\uninstall.log"
$p = Start-Process msiexec -ArgumentList "/x `"$($msi.FullName)`" /qn /l*v `"$root\dist\uninstall.log`"" -Wait -PassThru
Check "msiexec uninstall exit code 0 (got $($p.ExitCode))" ($p.ExitCode -eq 0)
Check 'install folder gone' (-not (Test-Path $dir))
Check 'Start-menu shortcut gone' (-not (Test-Path $lnk))
Check '.mpp association gone' (-not (Test-Path "$cls\.mpp"))
Check 'MathPlusPlus.Document gone' (-not (Test-Path "$cls\MathPlusPlus.Document"))
Check 'HKCU\Software\Math++ gone' (-not (Test-Path 'HKCU:\Software\Math++'))
Check 'user PATH no longer has the folder' (-not (([Environment]::GetEnvironmentVariable('Path', 'User') -split ';') -contains ($dir + [char]92)))
Check 'Apps & features entry gone' ($null -eq (Get-ItemProperty 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*' | Where-Object DisplayName -eq 'Math++'))
"RESULT: $fail failure(s)"
