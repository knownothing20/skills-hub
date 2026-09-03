$WshShell = New-Object -ComObject WScript.Shell
$DesktopPath = [System.Environment]::GetFolderPath("Desktop")
$ShortcutPath = Join-Path $DesktopPath "Skills Hub.lnk"
$TargetExe = Join-Path $PSScriptRoot "dist-app\skills-hub.exe"

$Shortcut = $WshShell.CreateShortcut($ShortcutPath)
$Shortcut.TargetPath = $TargetExe
$Shortcut.WorkingDirectory = $PSScriptRoot
$Shortcut.Description = "Skills Hub - Agent Skills Manager"
$Shortcut.IconLocation = "$TargetExe,0"
$Shortcut.Save()

Write-Host "DESKTOP_SHORTCUT_CREATED: $ShortcutPath"
