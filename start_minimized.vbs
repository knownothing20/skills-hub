Set WshShell = CreateObject("WScript.Shell")
Set fso = CreateObject("Scripting.FileSystemObject")
currentDir = fso.GetParentFolderName(WScript.ScriptFullName)

exeRelease = currentDir & "\src-tauri\target\release\skills-hub.exe"
exeDist = currentDir & "\dist-app\skills-hub.exe"

If fso.FileExists(exeRelease) Then
    WshShell.Run """" & exeRelease & """ --minimized", 0, False
ElseIf fso.FileExists(exeDist) Then
    WshShell.Run """" & exeDist & """ --minimized", 0, False
Else
    WshShell.Run "cmd /c cd /d """ & currentDir & """ && npm run tauri:dev -- -- --minimized", 0, False
End If
