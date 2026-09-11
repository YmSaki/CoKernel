@echo off
setlocal

powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "$p = Start-Process powershell.exe -Verb RunAs -PassThru -Wait -ArgumentList '-NoProfile -ExecutionPolicy Bypass -File ""%~dp0install-elevated.ps1""'; exit $p.ExitCode"
set "exit_code=%ERRORLEVEL%"

echo.
if "%exit_code%"=="0" (
  echo CoKernel installer finished successfully.
) else (
  echo CoKernel installer exited with code %exit_code%.
  echo.
  echo Diagnostic log: %LOCALAPPDATA%\CoKernel\install.log
  echo ---------------- CoKernel diagnostic tail ----------------
  powershell.exe -NoProfile -Command "$p = Join-Path $env:LOCALAPPDATA 'CoKernel\install.log'; if (Test-Path -LiteralPath $p) { Get-Content -LiteralPath $p -Tail 100 } else { Write-Host 'No diagnostic log was created. The failure happened before the elevated installer wrapper started.' }"
  echo ----------------------------------------------------------
)
echo.
pause
exit /b %exit_code%
