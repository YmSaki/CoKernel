@echo off
setlocal

powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "$p = Start-Process powershell.exe -Verb RunAs -PassThru -Wait -ArgumentList '-NoProfile -ExecutionPolicy Bypass -File ""%~dp0install.ps1""'; exit $p.ExitCode"
set "exit_code=%ERRORLEVEL%"

echo.
if "%exit_code%"=="0" (
  echo CoKernel installer finished successfully.
) else (
  echo CoKernel installer exited with code %exit_code%.
)
echo.
pause
exit /b %exit_code%
