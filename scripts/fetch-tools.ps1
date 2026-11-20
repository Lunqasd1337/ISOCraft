<#
.SYNOPSIS
  Downloads and verifies open-source binary tools (wimlib-imagex + MSYS2 xorriso)
  into src-tauri/resources/bin/ (gitignored).
#>
[CmdletBinding()]
param(
  [switch]$Force
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
$BinRoot = Join-Path $RepoRoot "src-tauri\resources\bin"
$WimlibDir = Join-Path $BinRoot "wimlib"
$XorrisoDir = Join-Path $BinRoot "xorriso"
$TempDir = Join-Path $env:TEMP ("isocraft-tools-" + [guid]::NewGuid().ToString("N"))

$Headers = @{ "User-Agent" = "ISOCraft-ToolFetcher/1.0" }

$WimlibArchive = @{
  Name   = "wimlib-1.14.5-windows-x86_64-bin.zip"
  Url    = "https://wimlib.net/downloads/wimlib-1.14.5-windows-x86_64-bin.zip"
  Sha256 = "2f446d6fa3866582175f1a22a7be198eeee0aec7aba5b4e04ad25c99eae2d265"
}

$XorrisoPackages = @(
  @{
    Name   = "xorriso-1.5.8.2-1-x86_64.pkg.tar.zst"
    Url    = "https://repo.msys2.org/msys/x86_64/xorriso-1.5.8.2-1-x86_64.pkg.tar.zst"
    Sha256 = "d97aed8ea0be8a247af5487aa3fc0790fcf488da9fc5c4964e7d73e5542b1036"
  },
  @{
    Name   = "msys2-runtime-3.6.12-1-x86_64.pkg.tar.zst"
    Url    = "https://repo.msys2.org/msys/x86_64/msys2-runtime-3.6.12-1-x86_64.pkg.tar.zst"
    Sha256 = "c0dbaa624e29be6d08cf5c65609dca6c749431109e4572bf446a3ca9b4c4fd7c"
  },
  @{
    Name   = "libiconv-1.19-1-x86_64.pkg.tar.zst"
    Url    = "https://repo.msys2.org/msys/x86_64/libiconv-1.19-1-x86_64.pkg.tar.zst"
    Sha256 = "0fa55ea2a6ccf97cf8c58b24b2615815e15e16e6e4e888091c263c2c83c5313d"
  },
  @{
    Name   = "zlib-1.4-1-x86_64.pkg.tar.zst"
    Url    = "https://repo.msys2.org/msys/x86_64/zlib-1.4-1-x86_64.pkg.tar.zst"
    Sha256 = "bafd3abf8b773ec28b6571d465a53a4e7afed4824cc1eec52f3bfbd6fa556ab9"
  },
  @{
    Name   = "libbz2-1.0.8-4-x86_64.pkg.tar.zst"
    Url    = "https://repo.msys2.org/msys/x86_64/libbz2-1.0.8-4-x86_64.pkg.tar.zst"
    Sha256 = "89fb0e1478b22b80effda55ab4ae7549388d01245d3841a096d8a2d63236c2a1"
  },
  @{
    Name   = "libreadline-8.3.006-2-x86_64.pkg.tar.zst"
    Url    = "https://repo.msys2.org/msys/x86_64/libreadline-8.3.006-2-x86_64.pkg.tar.zst"
    Sha256 = "e6c44cca4dcfd3821a4bcfca36bfef244af184e6963076bfcb15727001bfe8d2"
  },
  @{
    Name   = "ncurses-6.6-2-x86_64.pkg.tar.zst"
    Url    = "https://repo.msys2.org/msys/x86_64/ncurses-6.6-2-x86_64.pkg.tar.zst"
    Sha256 = "828c92f09b17f00d3b49362fcfa4affa58e5e50e238f290ea36eb7a690da3acb"
  }
)

function Get-VerifiedFile {
  param(
    [string]$Url,
    [string]$DestPath,
    [string]$ExpectedSha256
  )

  Write-Host "Downloading $(Split-Path $DestPath -Leaf)..."
  Invoke-WebRequest -Uri $Url -OutFile $DestPath -Headers $Headers -UseBasicParsing
  $actual = (Get-FileHash -Path $DestPath -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($actual -ne $ExpectedSha256.ToLowerInvariant()) {
    throw "SHA-256 mismatch for $DestPath. Expected: $ExpectedSha256, Actual: $actual"
  }
  Write-Host "  SHA-256 OK: $actual"
}

try {
  New-Item -ItemType Directory -Path $TempDir -Force | Out-Null

  # 1. wimlib-imagex
  $wimlibExe = Join-Path $WimlibDir "wimlib-imagex.exe"
  if ($Force -or -not (Test-Path $wimlibExe)) {
    if (Test-Path $WimlibDir) { Remove-Item $WimlibDir -Recurse -Force }
    New-Item -ItemType Directory -Path $WimlibDir -Force | Out-Null

    $zipPath = Join-Path $TempDir $WimlibArchive.Name
    Get-VerifiedFile -Url $WimlibArchive.Url -DestPath $zipPath -ExpectedSha256 $WimlibArchive.Sha256

    $wimExtract = Join-Path $TempDir "wimlib"
    New-Item -ItemType Directory -Path $wimExtract -Force | Out-Null
    tar.exe -xf $zipPath -C $wimExtract

    foreach ($item in @("wimlib-imagex.exe", "libwim-15.dll", "COPYING", "COPYING.GPLv3", "COPYING.LGPLv3")) {
      $src = Join-Path $wimExtract $item
      if (Test-Path $src) {
        Copy-Item -Path $src -Destination $WimlibDir -Force
      }
    }
    Write-Host "Installed wimlib-imagex to $WimlibDir"
  } else {
    Write-Host "wimlib-imagex already present at $wimlibExe (use -Force to re-download)."
  }

  # 2. xorriso (MSYS2 x86_64)
  $xorrisoExe = Join-Path $XorrisoDir "xorriso.exe"
  if ($Force -or -not (Test-Path $xorrisoExe)) {
    if (Test-Path $XorrisoDir) { Remove-Item $XorrisoDir -Recurse -Force }
    New-Item -ItemType Directory -Path $XorrisoDir -Force | Out-Null

    $msysExtract = Join-Path $TempDir "msys"
    New-Item -ItemType Directory -Path $msysExtract -Force | Out-Null

    foreach ($pkg in $XorrisoPackages) {
      $pkgPath = Join-Path $TempDir $pkg.Name
      Get-VerifiedFile -Url $pkg.Url -DestPath $pkgPath -ExpectedSha256 $pkg.Sha256
      tar.exe -xf $pkgPath -C $msysExtract 2>$null
    }

    Copy-Item -Path (Join-Path $msysExtract "usr\bin\xorriso.exe") -Destination $XorrisoDir -Force
    Get-ChildItem -Path (Join-Path $msysExtract "usr\bin") -Filter "msys-*.dll" | ForEach-Object {
      Copy-Item -Path $_.FullName -Destination $XorrisoDir -Force
    }

    $licSrc = Join-Path $msysExtract "usr\share\licenses"
    if (Test-Path $licSrc) {
      $licDest = Join-Path $XorrisoDir "licenses"
      Copy-Item -Path $licSrc -Destination $licDest -Recurse -Force
    }
    Write-Host "Installed xorriso + MSYS2 runtime DLLs to $XorrisoDir"
  } else {
    Write-Host "xorriso already present at $xorrisoExe (use -Force to re-download)."
  }

  Write-Host "`nAll tools verified and ready."
} finally {
  if (Test-Path $TempDir) {
    Remove-Item -Path $TempDir -Recurse -Force -ErrorAction SilentlyContinue
  }
}
