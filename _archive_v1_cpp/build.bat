@echo off
setlocal

set CONFIG=%1
if "%CONFIG%"=="" set CONFIG=Debug

echo Building %CONFIG% configuration...
cmake --build build --config %CONFIG% --parallel

if %ERRORLEVEL% EQU 0 (
    echo Build succeeded!
    if "%2"=="run" (
        echo Running GameEngine...
        build\bin\%CONFIG%\GameEngine.exe
    )
) else (
    echo Build failed!
    exit /b 1
)

endlocal 