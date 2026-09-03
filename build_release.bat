@echo off
chcp 65001 >nul
echo ======================================================
echo 正在进入 MSVC C++ 编译环境并进行 Skills Hub 极限优化编译...
echo ======================================================

set VCVARS="C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\VC\Auxiliary\Build\vcvars64.bat"

if exist %VCVARS% (
    call %VCVARS%
) else (
    echo [警告] 未找到 vcvars64.bat，将尝试直接使用系统环境
)

set PATH=%USERPROFILE%\.cargo\bin;%PATH%
set RUSTUP_DIST_SERVER=https://rsproxy.cn
set RUSTUP_UPDATE_ROOT=https://rsproxy.cn/rustup

cd /d "%~dp0"

echo.
echo [1/3] 构建前端生产包 (npm run build)...
call npm run build
if %ERRORLEVEL% NEQ 0 (
    echo [错误] 前端构建失败！
    exit /b %ERRORLEVEL%
)

echo.
echo [2/3] 编译 Rust 后端与系统托盘 Release 极致优化包...
cd /d "%~dp0src-tauri"
cargo build --release
if %ERRORLEVEL% NEQ 0 (
    echo [错误] 后端 Rust 编译失败！
    exit /b %ERRORLEVEL%
)

echo.
echo [3/3] 整理便携运行目录 (dist-app)...
cd /d "%~dp0"
if not exist "dist-app" mkdir "dist-app"
copy /y "%~dp0src-tauri\target\release\skills-hub.exe" "%~dp0dist-app\skills-hub.exe"

echo.
echo ======================================================
echo [成功] Skills Hub 编译完成！
echo 可执行程序位于: %~dp0dist-app\skills-hub.exe
echo 可直接双击 start_hub.bat 启动，或双击 start_minimized.vbs 静默常驻托盘。
echo ======================================================
