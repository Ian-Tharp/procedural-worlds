param(
    [Parameter(Position=0)]
    [ValidateSet('Debug', 'Release')]
    [string]$Configuration = 'Debug',
    
    [switch]$Run,
    [switch]$Clean
)

$BuildDir = "build"
$BinDir = "$BuildDir\bin\$Configuration"
$Executable = "GameEngine.exe"
$VcpkgToolchain = "C:/vcpkg/scripts/buildsystems/vcpkg.cmake"

# Clean build if requested
if ($Clean) {
    Write-Host "Cleaning build directory..." -ForegroundColor Yellow
    if (Test-Path $BuildDir) {
        Remove-Item -Recurse -Force $BuildDir
    }
    
    # Regenerate build files
    Write-Host "Regenerating build files..." -ForegroundColor Yellow
    cmake -B $BuildDir -S . -G "Visual Studio 17 2022" -DCMAKE_TOOLCHAIN_FILE="$VcpkgToolchain"
}

# Build the project
Write-Host "Building $Configuration configuration..." -ForegroundColor Green
cmake --build $BuildDir --config $Configuration --parallel

# Check if build succeeded
if ($LASTEXITCODE -eq 0) {
    Write-Host "Build succeeded!" -ForegroundColor Green
    
    # Run if requested
    if ($Run) {
        $ExePath = Join-Path $BinDir $Executable
        if (Test-Path $ExePath) {
            Write-Host "Running $ExePath..." -ForegroundColor Cyan
            & $ExePath
        } else {
            Write-Host "Executable not found at: $ExePath" -ForegroundColor Red
        }
    }
} else {
    Write-Host "Build failed!" -ForegroundColor Red
    exit 1
} 