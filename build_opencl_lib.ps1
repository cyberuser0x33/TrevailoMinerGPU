$msvcPath = "H:\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC\14.41.34120\bin\HostX64\x64"
$dumpbin = "$msvcPath\dumpbin.exe"
$libexe = "$msvcPath\lib.exe"
$dllPath = "C:\Windows\System32\OpenCL.dll"

if (-not (Test-Path $dllPath)) { Write-Host "OpenCL.dll NOT FOUND!"; exit 1 }

$dumpbinOutput = & $dumpbin /EXPORTS $dllPath 2>&1
$defContent = @("LIBRARY OpenCL", "EXPORTS")
$parsing = $false

foreach ($line in $dumpbinOutput) {
    if ($line -match "ordinal hint RVA      name") { $parsing = $true; continue }
    if ($parsing -and $line -match "^\s+\d+\s+[A-F0-9]+\s+[A-F0-9]+\s+([a-zA-Z0-9_]+)") {
        $defContent += $matches[1]
    }
}

$defContent | Out-File "OpenCL.def" -Encoding ascii

& $libexe /def:OpenCL.def /out:OpenCL.lib /machine:x64
Write-Host "OpenCL.lib generated!"
