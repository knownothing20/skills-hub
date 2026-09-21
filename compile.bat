@echo off
call "C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
set PATH=%USERPROFILE%\.cargo\bin;%PATH%
set RUSTUP_DIST_SERVER=https://rsproxy.cn
set RUSTUP_UPDATE_ROOT=https://rsproxy.cn/rustup
cd /d "D:\GitHub\skill-hub"
call npx tauri build --no-bundle
if %ERRORLEVEL% NEQ 0 exit /b %ERRORLEVEL%
if not exist "dist-app" mkdir "dist-app"
copy /y "D:\GitHub\skill-hub\src-tauri\target\release\skills-hub.exe" "D:\GitHub\skill-hub\dist-app\skills-hub.exe"
echo BUILD SUCCESS
