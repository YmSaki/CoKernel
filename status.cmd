@echo off
setlocal

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0runtime.ps1" -Action Status
set "exit_code=%ERRORLEVEL%"

echo.
pause
exit /b %exit_code%
