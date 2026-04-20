$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
if (Test-Path $vswhere) {
    $vsPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($vsPath) {
        $msvcDir = Get-ChildItem "$vsPath\VC\Tools\MSVC" | Sort-Object Name -Descending | Select-Object -First 1
        if ($msvcDir) {
            $msvcPath = "$($msvcDir.FullName)\bin\HostX64\x64"
        }
    }
}

if (-not $msvcPath -or -not (Test-Path "$msvcPath\dumpbin.exe")) {
    Write-Host "Visual Studio MSVC x64 tools not found!"
    exit 1
}

$dumpbin = "$msvcPath\dumpbin.exe"
$libexe = "$msvcPath\lib.exe"
$dllPath = "C:\Windows\System32\OpenCL.dll"

if (-not (Test-Path $dllPath)) { Write-Host "OpenCL.dll NOT FOUND!"; exit 1 }

$dumpbinOutput = & $dumpbin /EXPORTS $dllPath 2>&1
$defContent = @("LIBRARY OpenCL", "EXPORTS")

foreach ($line in $dumpbinOutput) {
    if ($line -match "^\s+\d+\s+[A-F0-9]+\s+[A-F0-9]+\s+([a-zA-Z0-9_]+)") {
        $defContent += $matches[1]
    }
}

$defContent | Out-File "OpenCL.def" -Encoding ascii

& $libexe /def:OpenCL.def /out:OpenCL.lib /machine:x64
Write-Host "OpenCL.lib generated!"
