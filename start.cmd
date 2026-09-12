@echo off
setlocal

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0runtime.ps1" -Action Start
set "exit_code=%ERRORLEVEL%"
if not "%exit_code%"=="0" goto failed

echo.
echo Starting CoKernel services inside the dedicated WSL runtime...
wsl.exe -d CoKernel --cd / -- bash -lc "cd ~/src/CoKernel && ./up.sh"
set "exit_code=%ERRORLEVEL%"
if not "%exit_code%"=="0" goto failed

echo.
echo Verifying Windows localhost access...
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0verify-windows-loopback.ps1"
set "exit_code=%ERRORLEVEL%"
if not "%exit_code%"=="0" goto failed

echo.
echo CoKernel is running persistently for this Windows login session.
echo JupyterLab: http://localhost:8888
goto done

:failed
echo.
echo CoKernel start failed with code %exit_code%.

:done
echo.
pause
exit /b %exit_code%
