$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [Console]::OutputEncoding
Set-Location (Join-Path $PSScriptRoot '..')
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw 'Cargo is missing. Install Rust >=1.75 from https://rustup.rs/' }
$clang = if ($env:LISAN_CLANG) { $env:LISAN_CLANG } else { 'clang' }
& $clang --version
if ($LASTEXITCODE -ne 0) { throw 'Clang preflight failed' }
cargo test --locked --all-targets
if ($LASTEXITCODE -ne 0) { throw 'Cargo tests failed' }
cargo build --locked --release
if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }
$sampleDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $sampleDir | Out-Null
try {
  $sample = Join-Path $sampleDir 'native-sample.exe'
  & '.\target\release\lisan.exe' --native 'examples\native.lsn' --output $sample
  if ($LASTEXITCODE -ne 0) { throw 'Native compilation failed' }
  $actual = & $sample
  if ($LASTEXITCODE -ne 0) { throw 'Native execution failed' }
  if (($actual -join "`n") -ne "0`n1`n9`n16`nانتهى") { throw "Unexpected native output: $actual" }
} finally { Remove-Item -Recurse -Force $sampleDir }
