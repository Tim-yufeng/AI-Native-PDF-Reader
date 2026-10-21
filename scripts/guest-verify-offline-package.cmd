@echo off
rem Guest-side launcher for the offline WebView2 validation run.
rem Started by src-tauri/installer/m8-offline-package.wsb as the sandbox logon command.
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "C:\M8ReadOnly\Package\guest-verify.ps1"
endlocal
