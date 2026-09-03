@echo off
chcp 65001 >nul
echo 正在为 Skills Hub 配置 Windows 开机自启常驻后台...

set TARGET_VBS=%~dp0start_minimized.vbs
set SHORTCUT_PATH=%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\SkillsHub.lnk

powershell -NoProfile -Command "$ws = New-Object -ComObject WScript.Shell; $s = $ws.CreateShortcut('%SHORTCUT_PATH%'); $s.TargetPath = '%TARGET_VBS%'; $s.WorkingDirectory = '%~dp0'; $s.Description = 'Skills Hub Agent Skills Manager'; $s.Save()"

if exist "%SHORTCUT_PATH%" (
    echo [成功] 已成功将 Skills Hub 添加到开机启动项！
    echo 每次开机后将自动静默常驻在系统托盘，不弹窗打扰。
) else (
    echo [失败] 创建快捷方式失败，请手动将 start_minimized.vbs 放入启动文件夹。
)
pause
