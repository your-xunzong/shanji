param(
    [string]$Version = "0.3.1"
)

$ErrorActionPreference = "Stop"

$repositoryRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$sourceExecutable = [System.IO.Path]::GetFullPath((Join-Path $repositoryRoot "src-tauri\target\release\shanji.exe"))
$artifactRoot = [System.IO.Path]::GetFullPath((Join-Path $repositoryRoot "artifacts"))
$portableDirectory = [System.IO.Path]::GetFullPath((Join-Path $artifactRoot "shanji-$Version-windows-x64-portable"))
$archivePath = [System.IO.Path]::GetFullPath((Join-Path $artifactRoot "shanji-$Version-windows-x64-portable.zip"))

if (-not $portableDirectory.StartsWith($artifactRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Portable output escaped the artifact directory."
}

if (-not (Test-Path -LiteralPath $sourceExecutable)) {
    throw "Release executable not found. Run 'pnpm tauri build --bundles nsis' first."
}

New-Item -ItemType Directory -Force -Path $portableDirectory | Out-Null
Copy-Item -LiteralPath $sourceExecutable -Destination (Join-Path $portableDirectory "shanji.exe") -Force
New-Item -ItemType File -Force -Path (Join-Path $portableDirectory "portable.flag") | Out-Null

Compress-Archive -LiteralPath (Join-Path $portableDirectory "shanji.exe"), (Join-Path $portableDirectory "portable.flag") -DestinationPath $archivePath -CompressionLevel Optimal -Force

Get-Item -LiteralPath $archivePath | Select-Object FullName, Length, LastWriteTime
