@echo off
chcp 65001 >nul
title Skills Hub 启动助手

set APP_EXE=%~dp0src-tauri\target\release\skills-hub.exe
if not exist "%APP_EXE%" (
    set APP_EXE=%~dp0dist-app\skills-hub.exe
)

if not exist "%APP_EXE%" (
    echo [提示] 尚未检测到编译好的可执行程序，正在通过 npm run tauri:dev 启动开发模式...
    cd /d "%~dp0"
    npm run tauri:dev
) else (
    echo [成功] 正在启动 Skills Hub...
    start "" "%APP_EXE%"
)
