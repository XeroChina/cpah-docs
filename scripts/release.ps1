[CmdletBinding()]
param(
    [string]$CargoAbout = "cargo-about"
)

$ErrorActionPreference = "Stop"
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$packageJsonPath = Join-Path $projectRoot "package.json"
$cargoTomlPath = Join-Path $projectRoot "src-tauri\Cargo.toml"
$tauriConfigPath = Join-Path $projectRoot "src-tauri\tauri.conf.json"
$thirdPartyLicensesPath = Join-Path $projectRoot "THIRD_PARTY_LICENSES.md"

function Get-Sha256Hex([string]$Path) {
    $stream = [System.IO.File]::OpenRead($Path)
    try {
        $sha256 = [System.Security.Cryptography.SHA256]::Create()
        try {
            $hashBytes = $sha256.ComputeHash($stream)
        }
        finally {
            $sha256.Dispose()
        }
    }
    finally {
        $stream.Dispose()
    }
    return -join ($hashBytes | ForEach-Object { $_.ToString("x2") })
}

$packageVersion = (Get-Content -LiteralPath $packageJsonPath -Raw -Encoding utf8 | ConvertFrom-Json).version
$tauriVersion = (Get-Content -LiteralPath $tauriConfigPath -Raw -Encoding utf8 | ConvertFrom-Json).version
$cargoContent = Get-Content -LiteralPath $cargoTomlPath -Raw -Encoding utf8
$cargoMatch = [regex]::Match($cargoContent, '(?ms)^\[package\].*?^version\s*=\s*"([^"]+)"')
if (-not $cargoMatch.Success) {
    throw "Cannot read package.version from src-tauri/Cargo.toml"
}
$cargoVersion = $cargoMatch.Groups[1].Value

if (($packageVersion -ne $cargoVersion) -or ($packageVersion -ne $tauriVersion)) {
    throw "Version mismatch: package.json=$packageVersion, Cargo.toml=$cargoVersion, tauri.conf.json=$tauriVersion"
}

Push-Location $projectRoot
try {
    node scripts/prepare-qpdf.mjs
    if ($LASTEXITCODE -ne 0) { throw "QPDF runtime preparation failed" }
    $licenseHashBefore = if (Test-Path -LiteralPath $thirdPartyLicensesPath -PathType Leaf) {
        Get-Sha256Hex $thirdPartyLicensesPath
    } else {
        $null
    }
    & (Join-Path $PSScriptRoot "generate-third-party-licenses.ps1") -CargoAbout $CargoAbout
    $licenseHashAfter = Get-Sha256Hex $thirdPartyLicensesPath
    if (-not $licenseHashBefore -or $licenseHashBefore -ne $licenseHashAfter) {
        throw "THIRD_PARTY_LICENSES.md was refreshed. Review and commit it, then run the release again."
    }

    cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
    if ($LASTEXITCODE -ne 0) { throw "Rust formatting check failed" }

    cargo fmt --manifest-path launcher/Cargo.toml --all -- --check
    if ($LASTEXITCODE -ne 0) { throw "Launcher formatting check failed" }

    npm.cmd run build
    if ($LASTEXITCODE -ne 0) { throw "Frontend production build failed" }

    npm.cmd run test:files
    if ($LASTEXITCODE -ne 0) { throw "File browser tests failed" }

    npm.cmd run test:tag-import
    if ($LASTEXITCODE -ne 0) { throw "Category import tests failed" }

    npm.cmd run test:i18n
    if ($LASTEXITCODE -ne 0) { throw "Internationalization tests failed" }

    cargo test --manifest-path src-tauri/Cargo.toml --all-targets
    if ($LASTEXITCODE -ne 0) { throw "Rust tests failed" }

    cargo test --manifest-path launcher/Cargo.toml --all-targets
    if ($LASTEXITCODE -ne 0) { throw "Launcher tests failed" }

    cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw "Clippy checks failed" }

    cargo clippy --manifest-path launcher/Cargo.toml --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw "Launcher Clippy checks failed" }

    npx.cmd tauri build --no-bundle
    if ($LASTEXITCODE -ne 0) { throw "Tauri release build failed" }

    cargo build --manifest-path launcher/Cargo.toml --release
    if ($LASTEXITCODE -ne 0) { throw "Launcher release build failed" }

    $cargoMetadataJson = cargo metadata --manifest-path src-tauri/Cargo.toml --format-version 1 --no-deps
    if ($LASTEXITCODE -ne 0) { throw "Cannot resolve Cargo build directory" }
    $cargoTargetDirectory = ($cargoMetadataJson | ConvertFrom-Json).target_directory
    $sourceExe = Join-Path $cargoTargetDirectory "release\cpah-docs.exe"
    if (-not (Test-Path -LiteralPath $sourceExe -PathType Leaf)) {
        throw "Release EXE not found: $sourceExe"
    }

    $launcherMetadataJson = cargo metadata --manifest-path launcher/Cargo.toml --format-version 1 --no-deps
    if ($LASTEXITCODE -ne 0) { throw "Cannot resolve launcher build directory" }
    $launcherTargetDirectory = ($launcherMetadataJson | ConvertFrom-Json).target_directory
    $launcherExe = Join-Path $launcherTargetDirectory "release\CPAH-Docs.exe"
    if (-not (Test-Path -LiteralPath $launcherExe -PathType Leaf)) {
        throw "Launcher EXE not found: $launcherExe"
    }

    $artifactDirectory = Join-Path $projectRoot "release"
    if (Test-Path -LiteralPath $artifactDirectory) {
        Remove-Item -LiteralPath $artifactDirectory -Recurse -Force
    }
    New-Item -ItemType Directory -Path $artifactDirectory -Force | Out-Null

    $portableFolderName = "CPAH-Docs-v$packageVersion-windows-x64"
    $portableRoot = Join-Path $artifactDirectory $portableFolderName
    $programDirectory = Join-Path $portableRoot "Program"
    $dataDirectory = Join-Path $portableRoot "Data"
    New-Item -ItemType Directory -Path $programDirectory -Force | Out-Null
    New-Item -ItemType Directory -Path $dataDirectory -Force | Out-Null

    Copy-Item -LiteralPath $launcherExe -Destination (Join-Path $portableRoot "CPAH-Docs.exe") -Force
    Copy-Item -LiteralPath $sourceExe -Destination (Join-Path $programDirectory "cpah-docs.exe") -Force
    [System.IO.File]::WriteAllBytes((Join-Path $portableRoot "portable"), @())
    Copy-Item -LiteralPath (Join-Path $projectRoot "LICENSE") -Destination (Join-Path $portableRoot "LICENSE.txt") -Force
    Copy-Item -LiteralPath $thirdPartyLicensesPath -Destination (Join-Path $portableRoot "THIRD_PARTY_LICENSES.md") -Force

    $artifactName = "CPAH-Docs-v$packageVersion-windows-x64-portable.zip"
    $artifactPath = Join-Path $artifactDirectory $artifactName
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [System.IO.Compression.ZipFile]::CreateFromDirectory(
        $portableRoot,
        $artifactPath,
        [System.IO.Compression.CompressionLevel]::Optimal,
        $true
    )
    Remove-Item -LiteralPath $portableRoot -Recurse -Force

    Copy-Item -LiteralPath (Join-Path $projectRoot "LICENSE") -Destination (Join-Path $artifactDirectory "LICENSE.txt") -Force
    Copy-Item -LiteralPath $thirdPartyLicensesPath -Destination (Join-Path $artifactDirectory "THIRD_PARTY_LICENSES.md") -Force

    $hash = Get-Sha256Hex $artifactPath
    Set-Content -LiteralPath (Join-Path $artifactDirectory "SHA256SUMS.txt") -Value "$hash  $artifactName" -Encoding ascii
    Write-Host "Release artifact: $artifactPath"
    Write-Host "SHA-256: $hash"
}
finally {
    Pop-Location
}
