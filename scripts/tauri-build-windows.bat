@echo off
setlocal

set "VCVARS=C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"
if not exist "%VCVARS%" (
  echo Visual Studio C++ build tools were not found at:
  echo %VCVARS%
  exit /b 1
)

call "%VCVARS%" >nul
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

cd /d "%~dp0\.."
npm run tauri:build -- %*
