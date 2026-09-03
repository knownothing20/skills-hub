$ErrorActionPreference = "Stop"

$vcvars = "C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if (Test-Path $vcvars) {
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

Set-Location $PSScriptRoot
npx tauri build --no-bundle
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$distDir = Join-Path $PSScriptRoot "dist-app"
if (-not (Test-Path $distDir)) {
    New-Item -ItemType Directory -Path $distDir -Force | Out-Null
}

$exeSource = Join-Path $PSScriptRoot "src-tauri\target\release\skills-hub.exe"
$exeTarget = Join-Path $distDir "skills-hub.exe"
Copy-Item -Path $exeSource -Destination $exeTarget -Force
Write-Host "BUILD_SUCCESS_EMBEDDED: $exeTarget"
