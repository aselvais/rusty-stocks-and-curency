<#
  Build a Windows release of Stock & Currency Tracker (MSVC target) and package a
  distributable .zip of the release binary. Requires Visual Studio 2022 with the
  "Desktop development with C++" workload.

  Usage:
    powershell -ExecutionPolicy Bypass -File scripts/release-windows.ps1

  Optional environment variables:
    RELEASE_VERSION  Version string for artifact names (default: Cargo version)
#>
$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

$Version = if ($env:RELEASE_VERSION) { $env:RELEASE_VERSION } else { (cargo pkgid).Split('@')[-1] }
$Product = "Stock & Currency Tracker"
$Bin     = "stock_currency_tracker"
$Target  = "x86_64-pc-windows-msvc"
$Dist    = "dist"

function Log($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }

Log "Ensuring target $Target is installed"
rustup target add $Target | Out-Null

Log "Building release binary (Windows / v$Version)"
cargo build --release --target $Target
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

Log "Packaging portable binary"
New-Item -ItemType Directory -Force -Path $Dist | Out-Null
$Stage = Join-Path "target" "$Target/release/dist"
Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
Copy-Item (Join-Path "target" "$Target/release/$Bin.exe") $Stage

$Zip = Join-Path $Dist "$Product-windows-x86_64-$Version.zip"
Remove-Item $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $Stage '*') -DestinationPath $Zip

Log "Optional: build an MSI with cargo-bundle"
if (Get-Command cargo-bundle -ErrorAction SilentlyContinue) {
  cargo bundle --format msi --target $Target
} else {
  Log "cargo-bundle not found; skipping MSI (run: cargo install cargo-bundle)"
}

Log "Artifacts:"
Get-ChildItem $Dist | Format-Table Name, Length
