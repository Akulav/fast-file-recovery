@echo off
rem Fast File Recovery — Windows dev entry.
rem   run.bat            build (default): release exe + setup, then launch
rem   run.bat quick      release exe only, then launch
rem   run.bat dev        debug build + run
rem   run.bat clean      cargo clean + delete dist\
rem   run.bat setup      build the setup only
setlocal
cd /d "%~dp0"
set MODE=%1
if "%MODE%"=="" set MODE=build

where cargo >nul 2>nul
if errorlevel 1 (
  echo Installing Rust with winget...
  winget install --id Rustlang.Rustup -e --silent --accept-package-agreements --accept-source-agreements
  set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
)

if /i "%MODE%"=="clean" (
  cargo clean
  if exist dist rmdir /s /q dist
  goto :eof
)
if /i "%MODE%"=="dev" (
  cargo run -p ffr-app -- --here
  goto :eof
)

cargo build --release -p ffr-app || goto :fail
if /i "%MODE%"=="quick" goto :launch

call :find_iscc
if not defined ISCC (
  echo Installing Inno Setup 6 with winget...
  winget install --id JRSoftware.InnoSetup -e --silent --accept-package-agreements --accept-source-agreements
  call :find_iscc
)
if not defined ISCC (
  echo Inno Setup not found. Install it from https://jrsoftware.org/isdl.php
  goto :fail
)
for /f "tokens=3 delims= " %%v in ('findstr /r /c:"^version = " Cargo.toml') do set VERSION=%%~v
"%ISCC%" /Q /DAppVersion=%VERSION% "/DSourceExe=%CD%\target\release\fast-file-recovery.exe" installer\ffr.iss || goto :fail
echo Built dist\FastFileRecovery-setup.exe
if /i "%MODE%"=="setup" goto :eof

:launch
start "" "target\release\fast-file-recovery.exe" --here
goto :eof

:find_iscc
set ISCC=
where iscc >nul 2>nul && set ISCC=iscc
if exist "%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe" set "ISCC=%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe"
if exist "%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe" set "ISCC=%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe"
exit /b 0

:fail
echo Build failed.
exit /b 1
