# Quick script to open the project in Visual Studio for debugging

$SolutionFile = "build\GameEngine.sln"
$VcpkgToolchain = "C:/vcpkg/scripts/buildsystems/vcpkg.cmake"

if (-not (Test-Path $SolutionFile)) {
    Write-Host "Solution file not found. Generating build files..." -ForegroundColor Yellow
    cmake -B build -S . -G "Visual Studio 17 2022" -DCMAKE_TOOLCHAIN_FILE="$VcpkgToolchain"
}

Write-Host "Opening Visual Studio..." -ForegroundColor Green
Start-Process $SolutionFile

Write-Host @"

Visual Studio Debugging Tips:
- Press F5 to start debugging (with breakpoints)
- Press Ctrl+F5 to run without debugging
- Set breakpoints by clicking in the left margin of code files
- Use Debug → Windows menu for debugging tools (Watch, Locals, Call Stack, etc.)

"@ -ForegroundColor Cyan 