@echo off
setlocal

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0sync-wsl-repo.ps1"
set "sync_exit=%ERRORLEVEL%"
if not "%sync_exit%"=="0" (
  echo.
  echo CoKernel could not synchronize the Windows checkout into the existing WSL environment.
  echo Exit code: %sync_exit%
  echo.
  pause
  exit /b %sync_exit%
)

powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "$p = Start-Process powershell.exe -Verb RunAs -PassThru -Wait -ArgumentList '-NoProfile -ExecutionPolicy Bypass -File ""%~dp0install-elevated.ps1""'; exit $p.ExitCode"
set "exit_code=%ERRORLEVEL%"

if not "%exit_code%"=="0" goto installer_failed

echo.
echo Verifying Windows localhost access to CoKernel...
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0verify-windows-loopback.ps1"
set "exit_code=%ERRORLEVEL%"
if not "%exit_code%"=="0" goto acceptance_failed

echo.
echo CoKernel installer finished successfully.
echo JupyterLab: http://localhost:8888
goto done

:installer_failed
echo.
echo CoKernel installer exited with code %exit_code%.
echo.
echo Diagnostic log: %LOCALAPPDATA%\CoKernel\install.log
echo ---------------- CoKernel diagnostic tail ----------------
powershell.exe -NoProfile -Command "$p = Join-Path $env:LOCALAPPDATA 'CoKernel\install.log'; if (Test-Path -LiteralPath $p) { Get-Content -LiteralPath $p -Tail 100 } else { Write-Host 'No diagnostic log was created. The failure happened before the elevated installer wrapper started.' }"
echo ----------------------------------------------------------
goto done

:acceptance_failed
echo.
echo CoKernel services started inside WSL, but Windows localhost verification failed.
echo Run update.cmd after fixing the reported bridge error, or rerun install.cmd for repair.

:done
echo.
pause
exit /b %exit_code%
