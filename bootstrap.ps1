$Repo = "lxl66566/bpm-rs"
$Version = if ($env:BPM_VERSION) { $env:BPM_VERSION } else { "latest" }

$Arch = if ([Environment]::Is64BitOperatingSystem) {
    if ((Get-CimInstance Win32_ComputerSystem).SystemType -match "ARM64") {
        "aarch64"
    } else {
        "x86_64"
    }
} else {
    Write-Error "Only 64-bit Windows is supported."
    exit 1
}

$Target = "$Arch-pc-windows-msvc"
$AssetName = "bin-package-manager-$Target.zip"

if ($Version -eq "latest") {
    $Url = "https://github.com/$Repo/releases/latest/download/$AssetName"
} else {
    $Url = "https://github.com/$Repo/releases/download/$Version/$AssetName"
}

$Tmp = Join-Path $env:TEMP "bpm-$(Get-Random)"
New-Item -ItemType Directory -Force -Path $Tmp | Out-Null
$Zip = Join-Path $Tmp "bpm.zip"
$ExtractDir = Join-Path $Tmp "extract"

Write-Host "Downloading bpm ($Target)..."
Invoke-WebRequest -Uri $Url -OutFile $Zip -UseBasicParsing

Write-Host "Extracting..."
Expand-Archive -Path $Zip -DestinationPath $ExtractDir

$BpmExe = Get-ChildItem -Recurse -Filter "bpm.exe" -Path $ExtractDir | Select-Object -First 1 -ExpandProperty FullName
if (-not $BpmExe) {
    Write-Error "bpm.exe not found in archive."
    exit 1
}

Write-Host "Installing bpm..."
& $BpmExe install --local $Zip bpm

Write-Host "bpm installed! Run 'bpm --help' to get started."
