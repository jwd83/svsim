@echo off
setlocal EnableExtensions EnableDelayedExpansion

set "ROOT_DIR=%~dp0"
if "%ROOT_DIR:~-1%"=="\" set "ROOT_DIR=%ROOT_DIR:~0,-1%"

pushd "%ROOT_DIR%" >nul
if errorlevel 1 (
    echo Failed to enter %ROOT_DIR% 1>&2
    exit /b 1
)

set "PARTS_DIR=parts"
set "RESULTS_DIR=results"
set "BINARY=target\debug\svsim.exe"

if not exist "%RESULTS_DIR%" mkdir "%RESULTS_DIR%"

echo Building svsim CLI...
cargo build -q -p svsim-cli --manifest-path Cargo.toml
if errorlevel 1 (
    popd
    exit /b 1
)

if not exist "%BINARY%" (
    echo Expected CLI binary at %ROOT_DIR%\%BINARY% after build 1>&2
    popd
    exit /b 1
)

set "STATUS=0"
set "DIR_COUNT=0"
set "AGGREGATE_ARGS="

for /f "delims=" %%D in ('dir /b /ad "%PARTS_DIR%"') do (
    if exist "%PARTS_DIR%\%%D\*.sv" if exist "%PARTS_DIR%\%%D\*.json" (
        set /a "DIR_COUNT+=1"
        set "OUTPUT=%RESULTS_DIR%\svsim_parts_%%D.json"
        set "AGGREGATE_ARGS=!AGGREGATE_ARGS! --json-test-dir %PARTS_DIR%\%%D"

        echo Running %ROOT_DIR%\%PARTS_DIR%\%%D
        "%BINARY%" --json-test-dir "%PARTS_DIR%\%%D" > "!OUTPUT!"
        if errorlevel 1 (
            echo Wrote !OUTPUT! ^(one or more suites failed^) 1>&2
            set "STATUS=1"
        ) else (
            echo Wrote !OUTPUT!
        )
    )
)

if "%DIR_COUNT%"=="0" (
    echo No runnable parts directories found under %PARTS_DIR% 1>&2
    popd
    exit /b 1
)

set "AGGREGATE_OUTPUT=%RESULTS_DIR%\svsim_parts_all.json"
echo Running aggregate corpus report
"%BINARY%" !AGGREGATE_ARGS! > "%AGGREGATE_OUTPUT%"
if errorlevel 1 (
    echo Wrote %AGGREGATE_OUTPUT% ^(one or more suites failed^) 1>&2
    set "STATUS=1"
) else (
    echo Wrote %AGGREGATE_OUTPUT%
)

set "EXIT_CODE=%STATUS%"
popd
exit /b %EXIT_CODE%
