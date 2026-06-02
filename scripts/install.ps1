$ErrorActionPreference = "Stop"

$Repo = if ($env:RUNAWARE_REPO) { $env:RUNAWARE_REPO } else { "ganeshpatro321/runaware" }
$Version = if ($env:RUNAWARE_VERSION) { $env:RUNAWARE_VERSION } else { "latest" }
$InstallDir = if ($env:RUNAWARE_INSTALL_DIR) { $env:RUNAWARE_INSTALL_DIR } else { Join-Path $HOME ".runaware\bin" }

if ($env:PROCESSOR_ARCHITECTURE -ne "AMD64") {
    throw "unsupported Windows architecture: $env:PROCESSOR_ARCHITECTURE"
}

$Target = "x86_64-pc-windows-msvc"
$Asset = "runaware-$Target.zip"

if ($Version -eq "latest") {
    $Url = "https://github.com/$Repo/releases/latest/download/$Asset"
} else {
    $Url = "https://github.com/$Repo/releases/download/$Version/$Asset"
}

$Tmp = New-Item -ItemType Directory -Force -Path (Join-Path ([System.IO.Path]::GetTempPath()) ("runaware-" + [System.Guid]::NewGuid()))
try {
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    $ZipPath = Join-Path $Tmp.FullName $Asset
    Write-Host "Downloading $Url"
    Invoke-WebRequest -Uri $Url -OutFile $ZipPath
    Expand-Archive -Path $ZipPath -DestinationPath $Tmp.FullName -Force
    Copy-Item -Path (Join-Path $Tmp.FullName "runaware-$Target\runaware.exe") -Destination (Join-Path $InstallDir "runaware.exe") -Force
    Write-Host "Installed runaware to $InstallDir\runaware.exe"
    Write-Host "Add $InstallDir to your PATH if runaware is not found."
} finally {
    Remove-Item -Recurse -Force $Tmp.FullName
}
