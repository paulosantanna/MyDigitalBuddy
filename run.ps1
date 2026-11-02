# Builds and launches MyDigitalBuddy on Windows.
# MSVC (link.exe) stays the default. Machines with only MinGW use the Rust GNU host.
$ErrorActionPreference = "Stop"
Set-Location -LiteralPath $PSScriptRoot

$gnu = "1.99.1-x86_64-pc-windows-gnu"
if (-not (Get-Command link.exe -ErrorAction SilentlyContinue)) {
    if (-not (Get-Command gcc.exe -ErrorAction SilentlyContinue)) {
        throw "Linker ausente. Instale o Visual Studio Build Tools com a carga C++, ou o MinGW-w64 (gcc no PATH)."
    }
    $installed = rustup toolchain list
    if ($installed -notmatch [regex]::Escape($gnu)) {
        rustup toolchain install $gnu
    }
    $env:RUSTUP_TOOLCHAIN = $gnu
}

cargo run --release --locked -- @args
