@echo off
setlocal

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0runtime.ps1" -Action Stop
set "exit_code=%ERRORLEVEL%"

echo.
if "%exit_code%"=="0" (
  echo CoKernel stopped.
) else (
  echo CoKernel stop failed with code %exit_code%.
)
echo.
pause
exit /b %exit_code%
