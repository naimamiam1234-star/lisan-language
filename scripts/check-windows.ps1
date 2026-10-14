$ErrorActionPreference = 'Stop'
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  Write-Error 'Cargo is missing. Install Rust from https://rustup.rs/'
  exit 1
}
Set-Location (Join-Path $PSScriptRoot '..')
cargo test --all-targets
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& '.\target\release\lisan.exe' 'examples\lists.lsn'
exit $LASTEXITCODE
