$ErrorActionPreference = "Continue"

$toolsDirs = @(
    "$HOME\.claude\skills",
    "$HOME\.codex\skills",
    "$HOME\.config\opencode\skills",
    "$HOME\.openclaw\skills",
    "$HOME\.codebuddy\skills",
    "$HOME\.workbuddy\skills",
    "$HOME\.kiro\skills",
    "$HOME\.trae-cn\skills",
    "$HOME\.gemini\skills",
    "$HOME\.codeium\windsurf\skills",
    "$HOME\.hermes\skills"
)

$syncedSkillNames = @(
    "browser-use",
    "frontend-ui-design",
    "gpt-researcher",
    "image-process",
    "metagpt",
    "programmatic-seo",
    "react-best-practices",
    "seo-auditor",
    "web-design-guidelines",
    "yt-dlp",
    "_shared",
    "aihot"
)

$removedCount = 0

foreach ($base in $toolsDirs) {
    if (-not (Test-Path $base)) { continue }
    
    foreach ($skillName in $syncedSkillNames) {
        $target = Join-Path $base $skillName
        if (Test-Path $target) {
            $item = Get-Item -LiteralPath $target -Force
            if ($item.Attributes.ToString() -match "ReparsePoint") {
                cmd /c rmdir "$target"
                if ($LASTEXITCODE -eq 0) {
                    $removedCount++
                    Write-Host "REMOVED_JUNCTION: $target"
                }
            } else {
                Write-Host "SKIPPED_REAL_DIR: $target"
            }
        }
    }
}

Write-Host "CLEANUP_SUCCESS: Total $removedCount junctions removed safely."
