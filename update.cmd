@echo off
setlocal

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0update.ps1"
set "exit_code=%ERRORLEVEL%"

echo.
if "%exit_code%"=="0" (
  echo CoKernel update finished successfully.
) else (
  echo CoKernel update failed with code %exit_code%.
)
echo.
pause
exit /b %exit_code%
