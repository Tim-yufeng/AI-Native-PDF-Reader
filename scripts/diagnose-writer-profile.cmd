@echo off
rem Read-only Sandbox diagnostic. This changes no execution-policy settings.
"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0diagnose-writer-profile.ps1"
echo.
echo Press any key to close.
pause >nul
