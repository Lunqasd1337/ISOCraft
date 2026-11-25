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

# Enforce TLS 1.2+ on Windows PowerShell 5.1 while preserving newer OS defaults
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$RepoRoot = Split-Path -Parent $PSScriptRoot
$BinRoot = Join-Path $RepoRoot "src-tauri\resources\bin"
$ManifestPath = Join-Path $BinRoot ".verified-sha256.json"
$WimlibDir = Join-Path $BinRoot "wimlib"
$XorrisoDir = Join-Path $BinRoot "xorriso"
$TarExe = Join-Path $env:SystemRoot "System32\tar.exe"
$TempDir = Join-Path $env:TEMP ("isocraft-tools-" + [guid]::NewGuid().ToString("N"))

if (-not (Test-Path -LiteralPath $TarExe)) {
  throw "Required Windows System32 tar.exe was not found at '$TarExe'."
}

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

function Get-ToolFileHashes {
  param(
    [string]$Dir,
    [string]$Prefix
  )

  $map = [ordered]@{}
  if (-not (Test-Path -LiteralPath $Dir)) {
    return $map
  }

  $files = Get-ChildItem -LiteralPath $Dir -File |
    Where-Object { $_.Extension -in @(".exe", ".dll") } |
    Sort-Object -Property Name
  foreach ($file in $files) {
    $hash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    $map["$Prefix/$($file.Name)"] = $hash
  }
  return $map
}

function Test-ManifestValid {
  param(
    [string]$Section,
    [hashtable]$ExpectedHashes,
    [string[]]$RequiredFiles,
    [string]$Dir
  )

  foreach ($req in $RequiredFiles) {
    $full = Join-Path $Dir $req
    if (-not (Test-Path -LiteralPath $full)) {
      return $false
    }
  }

  if (-not (Test-Path -LiteralPath $ManifestPath)) {
    return $false
  }

  try {
    $manifest = Get-Content -LiteralPath $ManifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
  } catch {
    return $false
  }

  $secObj = $manifest.$Section
  if ($null -eq $secObj) {
    return $false
  }

  $archiveProp = $secObj.archives
  if ($null -eq $archiveProp) {
    return $false
  }
  foreach ($key in $ExpectedHashes.Keys) {
    if ($archiveProp.$key -ne $ExpectedHashes[$key].ToLowerInvariant()) {
      return $false
    }
  }

  $filesObj = $secObj.files
  if ($null -eq $filesObj) {
    return $false
  }
  $actualHashes = Get-ToolFileHashes -Dir $Dir -Prefix $Section
  $recordedCount = ($filesObj.PSObject.Properties | Measure-Object).Count
  if ($recordedCount -eq 0 -or $recordedCount -ne $actualHashes.Count) {
    return $false
  }

  foreach ($relPath in $actualHashes.Keys) {
    if ($filesObj.$relPath -ne $actualHashes[$relPath]) {
      return $false
    }
  }

  return $true
}

$ManifestData = [ordered]@{}
if ((-not $Force) -and (Test-Path -LiteralPath $ManifestPath)) {
  try {
    $existingJson = Get-Content -LiteralPath $ManifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
    foreach ($prop in $existingJson.PSObject.Properties) {
      $ManifestData[$prop.Name] = $prop.Value
    }
  } catch {
    $ManifestData = [ordered]@{}
  }
}

try {
  New-Item -ItemType Directory -Path $TempDir -Force | Out-Null

  # 1. wimlib-imagex
  $wimlibExpectedArchives = @{ $WimlibArchive.Name = $WimlibArchive.Sha256 }
  $wimlibRequired = @("wimlib-imagex.exe", "libwim-15.dll")
  $wimlibValid = (-not $Force) -and (Test-ManifestValid -Section "wimlib" -ExpectedHashes $wimlibExpectedArchives -RequiredFiles $wimlibRequired -Dir $WimlibDir)

  if (-not $wimlibValid) {
    if (Test-Path -LiteralPath $WimlibDir) { Remove-Item -LiteralPath $WimlibDir -Recurse -Force }
    New-Item -ItemType Directory -Path $WimlibDir -Force | Out-Null

    $zipPath = Join-Path $TempDir $WimlibArchive.Name
    Get-VerifiedFile -Url $WimlibArchive.Url -DestPath $zipPath -ExpectedSha256 $WimlibArchive.Sha256

    $wimExtract = Join-Path $TempDir "wimlib"
    New-Item -ItemType Directory -Path $wimExtract -Force | Out-Null
    & $TarExe -xf $zipPath -C $wimExtract
    if ($LASTEXITCODE -ne 0) {
      throw "tar.exe failed extracting wimlib archive '$zipPath' (exit code $LASTEXITCODE)."
    }

    foreach ($item in @("wimlib-imagex.exe", "libwim-15.dll", "COPYING", "COPYING.GPLv3", "COPYING.LGPLv3")) {
      $src = Join-Path $wimExtract $item
      if (Test-Path -LiteralPath $src) {
        Copy-Item -LiteralPath $src -Destination $WimlibDir -Force
      }
    }

    foreach ($req in $wimlibRequired) {
      if (-not (Test-Path -LiteralPath (Join-Path $WimlibDir $req))) {
        throw "Required wimlib file '$req' is missing after extraction."
      }
    }

    $ManifestData["wimlib"] = [ordered]@{
      archives = [ordered]@{ $WimlibArchive.Name = $WimlibArchive.Sha256.ToLowerInvariant() }
      files    = (Get-ToolFileHashes -Dir $WimlibDir -Prefix "wimlib")
    }
    Write-Host "Installed wimlib-imagex to $WimlibDir"
  } else {
    Write-Host "wimlib-imagex verified at $WimlibDir (SHA-256 manifest OK; use -Force to re-download)."
  }

  # 2. xorriso (MSYS2 x86_64)
  $xorrisoExpectedArchives = [ordered]@{}
  foreach ($pkg in $XorrisoPackages) {
    $xorrisoExpectedArchives[$pkg.Name] = $pkg.Sha256.ToLowerInvariant()
  }
  $xorrisoRequired = @("xorriso.exe", "msys-2.0.dll")
  $xorrisoValid = (-not $Force) -and (Test-ManifestValid -Section "xorriso" -ExpectedHashes $xorrisoExpectedArchives -RequiredFiles $xorrisoRequired -Dir $XorrisoDir)

  if (-not $xorrisoValid) {
    if (Test-Path -LiteralPath $XorrisoDir) { Remove-Item -LiteralPath $XorrisoDir -Recurse -Force }
    New-Item -ItemType Directory -Path $XorrisoDir -Force | Out-Null

    $msysExtract = Join-Path $TempDir "msys"
    New-Item -ItemType Directory -Path $msysExtract -Force | Out-Null

    foreach ($pkg in $XorrisoPackages) {
      $pkgPath = Join-Path $TempDir $pkg.Name
      Get-VerifiedFile -Url $pkg.Url -DestPath $pkgPath -ExpectedSha256 $pkg.Sha256
      & $TarExe -xf $pkgPath -C $msysExtract
      if ($LASTEXITCODE -ne 0) {
        throw "tar.exe failed extracting MSYS2 package '$($pkg.Name)' (exit code $LASTEXITCODE)."
      }
    }

    $xorrisoSrc = Join-Path $msysExtract "usr\bin\xorriso.exe"
    if (-not (Test-Path -LiteralPath $xorrisoSrc)) {
      throw "Required xorriso.exe was not found in extracted MSYS2 archive."
    }
    Copy-Item -LiteralPath $xorrisoSrc -Destination $XorrisoDir -Force

    $msysDlls = Get-ChildItem -LiteralPath (Join-Path $msysExtract "usr\bin") -Filter "msys-*.dll"
    if ($msysDlls.Count -eq 0) {
      throw "No MSYS2 runtime DLLs (msys-*.dll) were found after extraction."
    }
    foreach ($dll in $msysDlls) {
      Copy-Item -LiteralPath $dll.FullName -Destination $XorrisoDir -Force
    }

    $licSrc = Join-Path $msysExtract "usr\share\licenses"
    if (Test-Path -LiteralPath $licSrc) {
      $licDest = Join-Path $XorrisoDir "licenses"
      Copy-Item -LiteralPath $licSrc -Destination $licDest -Recurse -Force
    }

    foreach ($req in $xorrisoRequired) {
      if (-not (Test-Path -LiteralPath (Join-Path $XorrisoDir $req))) {
        throw "Required xorriso runtime file '$req' is missing after extraction."
      }
    }

    $ManifestData["xorriso"] = [ordered]@{
      archives = $xorrisoExpectedArchives
      files    = (Get-ToolFileHashes -Dir $XorrisoDir -Prefix "xorriso")
    }
    Write-Host "Installed xorriso + MSYS2 runtime DLLs to $XorrisoDir"
  } else {
    Write-Host "xorriso verified at $XorrisoDir (SHA-256 manifest OK; use -Force to re-download)."
  }

  $ManifestData | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $ManifestPath -Encoding UTF8
  Write-Host "`nAll tools verified and ready (manifest: $ManifestPath)."
} finally {
  if (Test-Path -LiteralPath $TempDir) {
    Remove-Item -LiteralPath $TempDir -Recurse -Force -ErrorAction SilentlyContinue
  }
}
