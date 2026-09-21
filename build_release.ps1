$ErrorActionPreference = "Stop"
Write-Host "======================================================" -ForegroundColor Cyan
Write-Host "进入 MSVC x64 C++ 编译环境并进行 Release 编译..." -ForegroundColor Cyan
Write-Host "======================================================" -ForegroundColor Cyan

$vcvars = "C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\VC\Auxiliary\Build\vcvars64.bat"

if (Test-Path $vcvars) {
    Write-Host "正在加载 Visual Studio MSVC x64 环境变量..." -ForegroundColor Green
    $tempFile = [System.IO.Path]::GetTempFileName()
    cmd /c "call `"$vcvars`" >nul 2>&1 && set > `"$tempFile`""
    Get-Content $tempFile | ForEach-Object {
        if ($_ -match "^(.*?)=(.*)$") {
            [System.Environment]::SetEnvironmentVariable($matches[1], $matches[2], "Process")
        }
    }
    Remove-Item $tempFile -Force
}

$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
$env:RUSTUP_DIST_SERVER = "https://rsproxy.cn"
$env:RUSTUP_UPDATE_ROOT = "https://rsproxy.cn/rustup"

Write-Host "`n[1/2] 正在进行完整离线内嵌打包 (npx tauri build --no-bundle)..." -ForegroundColor Yellow
Set-Location $PSScriptRoot
npx tauri build --no-bundle
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "`n[2/2] 整理便携程序目录 (dist-app)..." -ForegroundColor Yellow
$distDir = Join-Path $PSScriptRoot "dist-app"
if (-not (Test-Path $distDir)) {
    New-Item -ItemType Directory -Path $distDir -Force | Out-Null
}

$exeSource = Join-Path $PSScriptRoot "src-tauri\target\release\skills-hub.exe"
$exeTarget = Join-Path $distDir "skills-hub.exe"

if (Test-Path $exeSource) {
    Copy-Item -Path $exeSource -Destination $exeTarget -Force
    Write-Host "`n======================================================" -ForegroundColor Green
    Write-Host "编译大功告成！" -ForegroundColor Green
    Write-Host "生成的可执行文件: $exeTarget" -ForegroundColor Cyan
    Write-Host "文件大小: $([Math]::Round((Get-Item $exeTarget).Length / 1MB, 2)) MB" -ForegroundColor Cyan
    Write-Host "======================================================" -ForegroundColor Green
} else {
    Write-Host "未能找到编译产物: $exeSource" -ForegroundColor Red
}
