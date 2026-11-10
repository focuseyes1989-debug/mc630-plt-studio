@echo off
setlocal
cd /d "%~dp0"
where cargo >nul 2>nul
if errorlevel 1 (
    echo Rust / Cargo is required. Install Rust, then reopen this launcher.
    pause
    exit /b 1
)
cargo run --locked --bin mc630-plt-studio -- %*
if errorlevel 1 pause
